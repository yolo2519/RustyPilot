/// A context guard that automatically executes cleanup functions when dropped.
///
/// This struct provides panic safety by ensuring that cleanup code is executed
/// even when the program panics. When a panic occurs, normal control flow is
/// interrupted and cleanup code may not run. By implementing `Drop`, this guard
/// ensures that registered cleanup functions are called when the guard goes out
/// of scope, regardless of whether the scope is exited normally or due to a panic.
///
/// # Examples
///
/// ```
/// use crate::utils::context::Context;
///
/// // Create a context with cleanup function
/// let _guard = Context::with(|| {
///     println!("Cleanup executed!");
/// });
///
/// // Cleanup will be called when _guard is dropped
/// ```
pub struct Context<AtExit: FnOnce()> {
    exit_cb: Option<AtExit>
}

impl<F: FnOnce()> Context<F> {
    /// Creates a new context guard without any cleanup function.
    pub fn new() -> Self { Self { exit_cb: None }}

    /// Creates a new context guard with the specified cleanup function.
    ///
    /// The cleanup function will be executed when the guard is dropped.
    pub fn with(f: F) -> Self { Self { exit_cb: Some(f) }}
}

impl<F: FnOnce()> Drop for Context<F> {
    fn drop(&mut self) {
        if let Some(f) = self.exit_cb.take() {
            f()
        }
    }
}
