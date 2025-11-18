use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};

/// A struct that implements Read and allows you to feed data into it
pub struct FeedableReader {
    buffer: Arc<Mutex<VecDeque<u8>>>,
}

impl FeedableReader {
    /// Create a new FeedableReader
    pub fn new() -> Self {
        FeedableReader {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Feed data into the reader
    pub fn feed(&self, data: &[u8]) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend(data);
    }
}

impl Read for FeedableReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        let n = std::cmp::min(buf.len(), buffer.len());

        for i in 0..n {
            buf[i] = buffer.pop_front().unwrap();
        }

        Ok(n)
    }
}

impl Clone for FeedableReader {
    fn clone(&self) -> Self {
        FeedableReader {
            buffer: Arc::clone(&self.buffer),
        }
    }
}
