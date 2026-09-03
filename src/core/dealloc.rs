/// Discards a heavy value in the background using Tokio's blocking thread pool if available,
/// or directly drops it synchronously if no runtime is active (e.g. in deterministic unit tests).
pub fn discard_in_background<T: Send + 'static>(value: T) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn_blocking(move || drop(value));
    } else {
        drop(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discard_in_background_without_runtime() {
        let val = vec![1, 2, 3, 4, 5];
        discard_in_background(val);
    }
}
