//! Secure password buffer for garlock
//!
//! Implements a password buffer that securely clears memory on drop
//! using the zeroize crate to prevent password remnants in memory.

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Default buffer capacity (should be enough for most passwords)
const DEFAULT_CAPACITY: usize = 256;

/// Secure password buffer
///
/// This buffer automatically zeroes its contents when dropped,
/// preventing password data from lingering in memory.
#[derive(ZeroizeOnDrop)]
pub struct Password {
    /// Raw UTF-8 bytes of the password
    buffer: Vec<u8>,
    /// Current length in bytes (not characters)
    len: usize,
}

impl Password {
    /// Create a new empty password buffer
    pub fn new() -> Self {
        Self {
            buffer: vec![0u8; DEFAULT_CAPACITY],
            len: 0,
        }
    }

    /// Create a password buffer with custom capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: vec![0u8; capacity],
            len: 0,
        }
    }

    /// Push a character onto the password buffer
    ///
    /// Returns `true` if the character was added, `false` if buffer is full.
    pub fn push(&mut self, c: char) -> bool {
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        let bytes = encoded.as_bytes();

        // Check if we have room
        if self.len + bytes.len() > self.buffer.len() {
            tracing::warn!("Password buffer full, cannot add character");
            return false;
        }

        // Copy bytes into buffer
        self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();

        tracing::trace!(len = self.len, "Character added to password buffer");
        true
    }

    /// Remove the last character from the password buffer
    ///
    /// Returns `true` if a character was removed, `false` if buffer was empty.
    pub fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }

        // Find the start of the last UTF-8 character
        // UTF-8 continuation bytes start with 10xxxxxx (0x80-0xBF)
        let mut char_start = self.len - 1;
        while char_start > 0 && (self.buffer[char_start] & 0xC0) == 0x80 {
            char_start -= 1;
        }

        // Zero out the removed bytes
        for i in char_start..self.len {
            self.buffer[i] = 0;
        }
        self.len = char_start;

        tracing::trace!(len = self.len, "Character removed from password buffer");
        true
    }

    /// Clear the entire password buffer
    ///
    /// Securely zeros all data in the buffer.
    pub fn clear(&mut self) {
        self.buffer[..self.len].zeroize();
        self.len = 0;
        tracing::trace!("Password buffer cleared");
    }

    /// Check if the password buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the current length in bytes
    pub fn len(&self) -> usize {
        self.len
    }

    /// Get the number of characters in the password
    pub fn char_count(&self) -> usize {
        self.as_str().chars().count()
    }

    /// Get the password as a string slice
    ///
    /// This is used when passing the password to PAM for authentication.
    pub fn as_str(&self) -> &str {
        // Safety: We only ever add valid UTF-8 characters via push()
        std::str::from_utf8(&self.buffer[..self.len]).unwrap_or("")
    }
}

impl Default for Password {
    fn default() -> Self {
        Self::new()
    }
}

// Manual Zeroize implementation for extra safety
impl Zeroize for Password {
    fn zeroize(&mut self) {
        self.buffer.zeroize();
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_pop() {
        let mut pw = Password::new();
        assert!(pw.is_empty());

        pw.push('h');
        pw.push('e');
        pw.push('l');
        pw.push('l');
        pw.push('o');

        assert_eq!(pw.as_str(), "hello");
        assert_eq!(pw.char_count(), 5);

        pw.pop();
        assert_eq!(pw.as_str(), "hell");

        pw.clear();
        assert!(pw.is_empty());
    }

    #[test]
    fn test_unicode() {
        let mut pw = Password::new();

        pw.push('日');
        pw.push('本');
        pw.push('語');

        assert_eq!(pw.as_str(), "日本語");
        assert_eq!(pw.char_count(), 3);
        assert_eq!(pw.len(), 9); // 3 bytes per character

        pw.pop();
        assert_eq!(pw.as_str(), "日本");
    }

    #[test]
    fn test_mixed_characters() {
        let mut pw = Password::new();

        pw.push('a');
        pw.push('é');
        pw.push('日');
        pw.push('!');

        assert_eq!(pw.char_count(), 4);

        pw.pop(); // !
        pw.pop(); // 日
        assert_eq!(pw.as_str(), "aé");
    }
}
