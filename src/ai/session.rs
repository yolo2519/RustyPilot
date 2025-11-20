use std::collections::HashMap;

use super::client::AiCommandSuggestion;

pub type SessionId = u64;

pub struct AiSession {
    pub id: SessionId,
    pub history: Vec<String>, // store text (easy version?)
    pub last_suggestion: Option<AiCommandSuggestion>,
}

pub struct AiSessionManager {
    sessions: HashMap<SessionId, AiSession>,
    current_id: SessionId,
    next_id: SessionId,
}

impl AiSessionManager {
    pub fn new() -> Self {
        let mut manager = Self {
            sessions: HashMap::new(),
            current_id: 1,
            next_id: 2,
        };
        manager.sessions.insert(
            1,
            AiSession {
                id: 1,
                history: Vec::new(),
                last_suggestion: None,
            },
        );
        manager
    }

    pub fn current_session(&self) -> &AiSession {
        self.sessions
            .get(&self.current_id)
            .expect("current session must exist")
    }

    pub fn current_session_mut(&mut self) -> &mut AiSession {
        self.sessions
            .get_mut(&self.current_id)
            .expect("current session must exist")
    }

    pub fn new_session(&mut self) -> SessionId {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(
            id,
            AiSession {
                id,
                history: Vec::new(),
                last_suggestion: None,
            },
        );
        self.current_id = id;
        id
    }
}
