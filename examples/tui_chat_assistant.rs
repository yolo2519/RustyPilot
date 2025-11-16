use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use async_openai::types::{
    ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

struct ChatMessage {
    role: MessageRole,
    content: String,
}

enum MessageRole {
    User,
    Assistant,
}

enum StreamUpdate {
    Chunk(String),
    Error(String),
    Complete(String),
}

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    input_cursor: usize, // Cursor position in input string
    scroll_offset: usize, // Scroll offset for chat history
    client: Client<async_openai::config::OpenAIConfig>,
    conversation_history: Vec<ChatCompletionRequestMessage>,
    is_processing: bool, // Whether AI is currently processing
    error_message: Option<String>,
    stream_rx: Option<mpsc::Receiver<StreamUpdate>>, // Channel to receive streaming updates
    current_assistant_msg_index: Option<usize>, // Index of the assistant message being streamed
}

impl App {
    fn new() -> Result<Self> {
        Ok(Self {
            messages: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            client: Client::new(),
            conversation_history: Vec::new(),
            is_processing: false,
            error_message: None,
            stream_rx: None,
            current_assistant_msg_index: None,
        })
    }

    async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            // Check for streaming updates
            self.process_stream_updates();

            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        if self.handle_key_event(key_event).await? {
                            break; // Exit requested
                        }
                    }
                    Event::Resize(..) => {
                        // Terminal was resized, will be handled in next draw
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<bool> {
        // Don't allow input while processing
        if self.is_processing {
            if key_event.code == KeyCode::Char('c') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(true); // Allow Ctrl+C to exit even during processing
            }
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true); // Exit
            }
            KeyCode::Enter => {
                self.send_message().await;
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    // Find the char boundary before cursor
                    let char_start = self.input.char_indices()
                        .nth(self.input_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.drain(char_start..);
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Delete => {
                if self.input_cursor < self.input.chars().count() {
                    // Find the char boundary at cursor
                    if let Some((char_start, _)) = self.input.char_indices().nth(self.input_cursor) {
                        // Find the end of this character
                        let char_end = self.input[char_start..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| char_start + i)
                            .unwrap_or(self.input.len());
                        self.input.drain(char_start..char_end);
                    }
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let char_count = self.input.chars().count();
                if self.input_cursor < char_count {
                    self.input_cursor += 1;
                }
            }
            KeyCode::Home => {
                self.input_cursor = 0;
            }
            KeyCode::End => {
                self.input_cursor = self.input.chars().count();
            }
            KeyCode::Up => {
                // Scroll chat history up (show earlier content)
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Down => {
                // Scroll chat history down (show later content, towards bottom)
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
            }
            KeyCode::PageUp => {
                // Scroll up by 5 (show earlier content)
                self.scroll_offset = self.scroll_offset.saturating_add(5);
            }
            KeyCode::PageDown => {
                // Scroll down by 5 (show later content, towards bottom)
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
            }
            KeyCode::Char(c) => {
                if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !key_event.modifiers.contains(KeyModifiers::ALT)
                {
                    // Find the byte position for character insertion
                    let insert_pos = self.input.char_indices()
                        .nth(self.input_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.input.len());
                    self.input.insert(insert_pos, c);
                    self.input_cursor += 1;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn send_message(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        // Clear error message
        self.error_message = None;

        // Add user message to display
        self.messages.push(ChatMessage {
            role: MessageRole::User,
            content: input.clone(),
        });

        // Clear input
        self.input.clear();
        self.input_cursor = 0;

        // Reset scroll to bottom (show latest content)
        self.scroll_offset = 0;

        // Send to OpenAI (non-blocking, streaming updates handled in main loop)
        self.is_processing = true;
        if let Err(e) = self.start_chat_stream(&input).await {
            self.is_processing = false;
            self.error_message = Some(format!("Error: {}", e));
        }
    }

    async fn start_chat_stream(
        &mut self,
        user_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Add user message to conversation history
        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(user_message)
            .build()?
            .into();
        self.conversation_history.push(user_msg);

        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5-nano")
            .messages(self.conversation_history.as_slice())
            .build()?;

        let mut stream = self.client.chat().create_stream(request).await?;

        // Create a channel to send updates to the UI thread
        let (tx, rx) = mpsc::channel::<StreamUpdate>();

        // Create assistant message placeholder
        let assistant_msg_index = self.messages.len();
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
        });
        self.current_assistant_msg_index = Some(assistant_msg_index);
        self.stream_rx = Some(rx);

        // Spawn a task to handle streaming
        tokio::spawn(async move {
            let mut ai_response = String::new();
            while let Some(result) = stream.next().await {
                match result {
                    Ok(response) => {
                        let mut chunk = String::new();
                        response.choices.iter().for_each(|chat_choice| {
                            if let Some(ref content) = chat_choice.delta.content {
                                chunk.push_str(content);
                                ai_response.push_str(content);
                            }
                        });
                        if !chunk.is_empty() {
                            let _ = tx.send(StreamUpdate::Chunk(chunk));
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(StreamUpdate::Error(format!("Error: {}", err)));
                        break;
                    }
                }
            }
            // Send completion marker with final response
            let _ = tx.send(StreamUpdate::Complete(ai_response));
        });

        Ok(())
    }

    fn process_stream_updates(&mut self) {
        let mut should_clear_stream = false;
        let mut final_response = None;
        
        if let Some(ref rx) = self.stream_rx {
            while let Ok(update) = rx.try_recv() {
                match update {
                    StreamUpdate::Chunk(chunk) => {
                        if let Some(idx) = self.current_assistant_msg_index {
                            if let Some(msg) = self.messages.get_mut(idx) {
                                msg.content.push_str(&chunk);
                            }
                        }
                    }
                    StreamUpdate::Error(err) => {
                        self.error_message = Some(err);
                        self.is_processing = false;
                        should_clear_stream = true;
                    }
                    StreamUpdate::Complete(response) => {
                        final_response = Some(response);
                        self.is_processing = false;
                        should_clear_stream = true;
                    }
                }
            }
        }
        
        // Clear stream after processing all updates to avoid borrow issues
        if should_clear_stream {
            self.stream_rx = None;
            self.current_assistant_msg_index = None;
            
            // Reset scroll to bottom when message completes
            // This ensures user sees the new complete message
            self.scroll_offset = 0;
            
            // Add AI response to conversation history if we have one
            if let Some(response) = final_response {
                if !response.trim().is_empty() {
                    if let Ok(assistant_msg) = ChatCompletionRequestAssistantMessageArgs::default()
                        .content(response.trim())
                        .build()
                    {
                        self.conversation_history.push(assistant_msg.into());
                    }
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Calculate input area height based on content (with Unicode width-based wrapping)
        let input_area_width = area.width.saturating_sub(4); // Account for borders
        // Use even width for height calculation to match wrapping logic
        let effective_width = ((input_area_width as usize / 2) * 2).max(2) as u16;
        let input_lines = if self.input.is_empty() {
            1
        } else if effective_width > 0 {
            let input_width = self.input.width();
            let max_width = effective_width as usize;
            (input_width + max_width - 1) / max_width
        } else {
            1
        };
        let input_height = (input_lines + 2).min(10) as u16; // +2 for borders, max 10 total

        // Split into chat area and input area
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),                    // Chat history (takes remaining space)
                Constraint::Length(input_height),      // Input area (dynamic height)
            ])
            .split(area);

        // Draw chat history
        self.draw_chat_history(chunks[0], frame);

        // Draw input area
        self.draw_input(chunks[1], frame);
    }

    fn draw_chat_history(&self, area: Rect, frame: &mut Frame) {
        let inner_area = {
            let temp_block = Block::default().borders(Borders::ALL);
            temp_block.inner(area)
        };

        // Render messages
        let mut all_lines = Vec::new();

        // Build all lines from messages
        for (idx, msg) in self.messages.iter().enumerate() {
            let role_label = match msg.role {
                MessageRole::User => "You:",
                MessageRole::Assistant => "ChatGPT:",
            };

            let role_style = match msg.role {
                MessageRole::User => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                MessageRole::Assistant => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            };

            // Check if this is the last message, it's an assistant message, and we're processing
            // Add role label with processing indicator if needed
            if self.is_processing && idx + 1 == self.messages.len()
                && matches!(msg.role, MessageRole::Assistant) {
                all_lines.push(Line::from(vec![
                    Span::styled(role_label, role_style),
                    Span::raw(" "),
                    Span::styled("...", Style::default().fg(Color::Yellow)),
                ]));
            } else {
                all_lines.push(Line::from(vec![
                    Span::styled(role_label, role_style),
                    Span::raw(" "),
                ]));
            }

            // Split message content into lines and add them
            for line in msg.content.lines() {
                all_lines.push(Line::from(Span::raw(line)));
            }

            // Add blank line between messages
            all_lines.push(Line::from(""));
        }

        // Show error message if any
        if let Some(ref error) = self.error_message {
            all_lines.push(Line::from(vec![
                Span::styled(error, Style::default().fg(Color::Red)),
            ]));
        }

        // Calculate total text lines (logical lines, not wrapped)
        let total_text_lines = all_lines.len();

        // Calculate line number widths
        let text_line_num_width = if total_text_lines == 0 {
            1
        } else {
            (total_text_lines as f64).log10().floor() as usize + 1
        }.max(3);

        // We'll calculate widget line number width after wrapping, so use a reasonable estimate for now
        // The actual width will be calculated after we know how many widget lines there are
        // For now, use the same width as text line numbers as an estimate
        let estimated_widget_line_num_width = text_line_num_width;
        let line_num_area_width = text_line_num_width + 3 + estimated_widget_line_num_width + 3; // "text_num │ " + "widget_num │ "
        let content_width = inner_area.width.saturating_sub(line_num_area_width as u16);

        // Manually wrap lines and track which are logical lines vs wrapped lines
        struct WrappedLine {
            text_line_num: Option<usize>, // None for wrapped continuation lines
            widget_line_num: usize,
            spans: Vec<Span<'static>>, // Store spans to preserve styling
        }

        let mut wrapped_lines = Vec::new();
        let mut widget_line_num = 0;

        for (text_line_idx, line) in all_lines.iter().enumerate() {
            // Get all spans from the line and convert to 'static
            // We'll preserve the original spans structure but convert to owned strings
            let original_spans: Vec<Span> = line.iter().cloned().collect();
            
            // Store spans with their text for wrapping calculation
            // We'll recreate spans with preserved styling after wrapping
            let mut span_data: Vec<(String, Style)> = Vec::new();
            for span in &original_spans {
                let text = format!("{}", span);
                // Extract style - Span doesn't expose style directly, so we'll need to
                // reconstruct it. For now, we'll try to detect if it's a styled span
                // by checking if the text matches known patterns
                let style = Style::default(); // Default for now - we'll improve this
                span_data.push((text, style));
            }
            
            // Convert to static spans for wrapping, preserving styles based on content
            let static_spans: Vec<Span<'static>> = span_data.iter()
                .map(|(text, _style)| {
                    if text == "You:" || text.starts_with("You:") {
                        Span::styled(text.clone(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                    } else if text == "ChatGPT:" || text.starts_with("ChatGPT:") {
                        Span::styled(text.clone(), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))
                    } else {
                        Span::raw(text.clone())
                    }
                })
                .collect();
            
            // Calculate total display width for wrapping (using Unicode width)
            let max_width = content_width as usize;
            
            // Build a single string from all spans to calculate total width
            let full_text: String = static_spans.iter()
                .map(|span| format!("{}", span))
                .collect();
            
            let total_width = full_text.width();
            
            // If line fits in one widget line, no wrapping needed
            if total_width <= max_width {
                wrapped_lines.push(WrappedLine {
                    text_line_num: Some(text_line_idx + 1),
                    widget_line_num: widget_line_num + 1,
                    spans: static_spans,
                });
                widget_line_num += 1;
            } else {
                // Need to wrap: split by display width
                let mut current_char_pos = 0;
                let mut is_first_wrap = true;
                
                while current_char_pos < full_text.len() {
                    // Calculate remaining text and its width
                    let remaining_text = &full_text[current_char_pos..];
                    let remaining_width = remaining_text.width();
                    
                    if remaining_width <= max_width {
                        // Last portion - take everything
                        let text_portion = remaining_text.to_string();
                        wrapped_lines.push(WrappedLine {
                            text_line_num: if is_first_wrap { Some(text_line_idx + 1) } else { None },
                            widget_line_num: widget_line_num + 1,
                            spans: vec![Span::raw(text_portion)],
                        });
                        widget_line_num += 1;
                        break;
                    }
                    
                    // Find break point by iterating through characters and tracking width
                    let mut break_char_pos = current_char_pos;
                    let mut current_width = 0;
                    let mut last_space_pos = None;
                    
                    for (char_idx, ch) in remaining_text.char_indices() {
                        // Calculate character display width using Unicode width
                        let char_width = ch.width().unwrap_or(1);
                        
                        if current_width + char_width > max_width {
                            // Would exceed width - use last space if available, otherwise break here
                            break_char_pos = if let Some(space_pos) = last_space_pos {
                                current_char_pos + space_pos + 1
                            } else {
                                current_char_pos + char_idx
                            };
                            break;
                        }
                        
                        current_width += char_width;
                        
                        // Track last space position for better word breaking
                        if ch == ' ' || ch == '\t' {
                            last_space_pos = Some(char_idx);
                        }
                    }
                    
                    // If we didn't find a break point, use the end
                    if break_char_pos == current_char_pos {
                        break_char_pos = full_text.len();
                    }
                    
                    // Extract the text portion
                    let text_portion = full_text[current_char_pos..break_char_pos].to_string();
                    
                    wrapped_lines.push(WrappedLine {
                        text_line_num: if is_first_wrap { Some(text_line_idx + 1) } else { None },
                        widget_line_num: widget_line_num + 1,
                        spans: vec![Span::raw(text_portion)],
                    });
                    
                    widget_line_num += 1;
                    current_char_pos = break_char_pos;
                    is_first_wrap = false;
                    
                    // Skip leading spaces on next line
                    while current_char_pos < full_text.len() {
                        let ch = full_text[current_char_pos..].chars().next().unwrap();
                        if ch == ' ' || ch == '\t' {
                            current_char_pos += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // Calculate widget line number width
        let widget_line_num_width = if widget_line_num == 0 {
            1
        } else {
            (widget_line_num as f64).log10().floor() as usize + 1
        }.max(3);

        // Calculate actual content width (after accounting for both line numbers)
        let actual_line_num_area_width = text_line_num_width + 3 + widget_line_num_width + 3; // "text_num │ " + "widget_num │ "
        let actual_content_width = inner_area.width.saturating_sub(actual_line_num_area_width as u16);

        // Apply scroll offset to widget lines
        let max_widget_lines = inner_area.height as usize;
        let start_widget_line = if widget_line_num <= max_widget_lines {
            0
        } else {
            let base_start = widget_line_num.saturating_sub(max_widget_lines);
            base_start.saturating_sub(self.scroll_offset)
        };
        let end_widget_line = (start_widget_line + max_widget_lines).min(widget_line_num);
        let visible_wrapped_lines: Vec<&WrappedLine> = wrapped_lines
            .iter()
            .skip(start_widget_line)
            .take(end_widget_line - start_widget_line)
            .collect();

        // Build lines with two line numbers
        let mut lines_with_numbers = Vec::new();
        for wrapped_line in visible_wrapped_lines {
            let mut spans = Vec::new();

            // First line number (text line number) - only show if not wrapped continuation
            if let Some(text_num) = wrapped_line.text_line_num {
                let text_num_str = format!("{:>width$}", text_num, width = text_line_num_width);
                spans.push(Span::styled(
                    format!("{} │ ", text_num_str),
                    Style::default().fg(Color::DarkGray)
                ));
            } else {
                // Wrapped continuation line - show spaces instead
                spans.push(Span::styled(
                    format!("{:>width$} │ ", " ", width = text_line_num_width),
                    Style::default().fg(Color::DarkGray)
                ));
            }

            // Second line number (widget line number) - always show
            let widget_num_str = format!("{:>width$}", wrapped_line.widget_line_num, width = widget_line_num_width);
            spans.push(Span::styled(
                format!("{} │ ", widget_num_str),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
            ));

            // Add content with preserved styling
            spans.extend(wrapped_line.spans.iter().cloned());

            lines_with_numbers.push(Line::from(spans));
        }

        // Create title with debug info
        let title = format!(
            " Chat History [{}x{}] content:{} {} text lines, {} widget lines ",
            inner_area.width,
            inner_area.height,
            actual_content_width,
            total_text_lines,
            widget_line_num
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));

        block.render(area, frame.buffer_mut());

        // Render directly to inner_area (we've already accounted for line numbers in spans)
        let paragraph = Paragraph::new(lines_with_numbers);
        paragraph.render(inner_area, frame.buffer_mut());
    }

    fn draw_input(&self, area: Rect, frame: &mut Frame) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Input (Enter to send, Ctrl+C to exit) ")
            .border_style(Style::default().fg(Color::Yellow));

        let inner_area = block.inner(area);
        block.render(area, frame.buffer_mut());

        // Show input text with cursor
        let display_text = if self.input.is_empty() {
            String::from("Type your message here...")
        } else {
            self.input.clone()
        };

        let style = if self.input.is_empty() {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
        } else {
            Style::default()
        };

        // Manual wrapping: split by Unicode display width, ignoring word boundaries
        // Use even width to avoid issues with wide characters (Chinese, etc.)
        let wrapped_lines = if inner_area.width > 0 {
            // Round down to even width to avoid odd-width issues with wide characters
            let max_width = (inner_area.width as usize / 2) * 2;
            let mut lines = Vec::new();
            let text = if self.input.is_empty() {
                &display_text
            } else {
                &self.input
            };
            
            // Split by Unicode width, not character count
            let mut current_pos = 0;
            let chars: Vec<char> = text.chars().collect();
            
            while current_pos < chars.len() {
                let mut line = String::new();
                let mut line_width = 0;
                
                while current_pos < chars.len() {
                    let ch = chars[current_pos];
                    let char_width = ch.width().unwrap_or(1);
                    
                    if line_width + char_width > max_width {
                        break;
                    }
                    
                    line.push(ch);
                    line_width += char_width;
                    current_pos += 1;
                }
                
                if line.is_empty() && current_pos < chars.len() {
                    // Single character is wider than max_width, add it anyway
                    line.push(chars[current_pos]);
                    current_pos += 1;
                }
                
                lines.push(line);
            }
            
            if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            }
        } else {
            vec![display_text]
        };

        let paragraph = Paragraph::new(wrapped_lines.join("\n"))
            .style(style);

        paragraph.render(inner_area, frame.buffer_mut());

        // Set cursor position in input (using Unicode width for proper display)
        if !self.input.is_empty() && inner_area.height > 0 && inner_area.width > 0 {
            // Get text before cursor using character index
            let text_before_cursor: String = self.input.chars()
                .take(self.input_cursor)
                .collect();
            
            // Use even width to match wrapping logic (round down to even)
            let max_width = (inner_area.width as usize / 2) * 2;
            let text_width = text_before_cursor.width();
            
            // Calculate which line and column the cursor is on (based on display width)
            let cursor_line = (text_width / max_width) as u16;
            let cursor_col = (text_width % max_width) as u16;
            
            // Ensure cursor is within visible area
            let cursor_x = inner_area.x + cursor_col.min(max_width as u16);
            let cursor_y = inner_area.y + cursor_line.min(inner_area.height.saturating_sub(1));
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("NOTE: Please make sure you have set your OPENAI_API_KEY environment variable.");
    println!("Starting TUI Chat Assistant...");

    let mut terminal = ratatui::init();

    // Create app
    let mut app = App::new()?;

    // Run the app
    let result = app.run(&mut terminal).await;

    ratatui::restore();
    result
}
