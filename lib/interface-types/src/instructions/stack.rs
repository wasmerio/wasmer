use std::{iter::Rev, vec::Drain};

pub(super) struct Stack {
    inner: Vec<u32>,
}

impl Stack {
    pub(super) fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(super) fn push(&mut self, item: u32) {
        self.inner.push(item);
    }

    pub(super) fn pop(&mut self) -> Option<u32> {
        self.inner.pop()
    }

    pub(super) fn pop_n(&mut self, n: usize) -> Rev<Drain<u32>> {
        self.inner.drain(self.inner.len() - n..).rev()
    }
}

#[cfg(test)]
mod tests {
    use super::Stack;

    #[test]
    fn test_is_empty() {
        let mut stack = Stack::new();
        assert_eq!(stack.is_empty(), true);

        stack.push(1);
        assert_eq!(stack.is_empty(), false);
    }

    #[test]
    fn test_push_pop() {
        let mut stack = Stack::new();
        stack.push(1);

        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.is_empty(), true);
    }

    #[test]
    fn test_pop_n() {
        let mut stack = Stack::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);
        stack.push(4);
        stack.push(5);
        stack.push(6);

        assert_eq!(stack.pop_n(1).collect::<Vec<_>>(), &[6]);
        assert_eq!(stack.pop_n(2).collect::<Vec<_>>(), &[5, 4]);
        assert_eq!(stack.pop_n(3).collect::<Vec<_>>(), &[3, 2, 1]);
        assert_eq!(stack.is_empty(), true);
    }
}
