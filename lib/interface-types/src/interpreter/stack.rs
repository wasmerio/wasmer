//! A very light and generic stack implementation, exposing only the
//! operations required by the interpreter.

/// The `Stackable` trait represents a small basic set of operations
/// required by the interpreter.
pub trait Stackable {
    /// The kind of item the stack holds.
    type Item;

    /// Checks whether the stack is empty.
    fn is_empty(&self) -> bool;

    /// Extracts a slice containing the entire stack.
    fn as_slice(&self) -> &[Self::Item];

    /// Appends one item to the end of the stack.
    fn push(&mut self, item: Self::Item);

    /// Removes the last item of the stack and returns it, `None` if
    /// the stack is empty.
    fn pop1(&mut self) -> Option<Self::Item>;

    /// Removes `n` elements from the end of the stack, `None` if the
    /// stack doesn't contain enough elements.
    /// Returned items are in reverse order: the last element comes
    /// last in the list.
    fn pop(&mut self, n: usize) -> Option<Vec<Self::Item>>;

    /// Peek the last item of the stack and returns a reference to it,
    /// `None` if the stack is empty.
    fn peek1(&self) -> Option<&Self::Item>;
}

/// A stack implementation of the `Stackable` trait, based on a vector.
#[derive(Debug, Default)]
pub struct Stack<T>
where
    T: Default + Clone,
{
    /// Inner structure holding the items.
    inner: Vec<T>,
}

impl<T> Stack<T>
where
    T: Default + Clone,
{
    /// Creates a new empty stack.
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

impl<T> Stackable for Stack<T>
where
    T: Default + Clone,
{
    type Item = T;

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn as_slice(&self) -> &[Self::Item] {
        self.inner.as_slice()
    }

    fn push(&mut self, item: Self::Item) {
        self.inner.push(item);
    }

    fn pop1(&mut self) -> Option<Self::Item> {
        self.inner.pop()
    }

    fn pop(&mut self, n: usize) -> Option<Vec<Self::Item>> {
        if self.inner.len() < n {
            None
        } else {
            let items = self
                .inner
                .drain(self.inner.len() - n..)
                .collect::<Vec<Self::Item>>();

            assert!(items.len() == n);

            Some(items)
        }
    }

    fn peek1(&self) -> Option<&Self::Item> {
        if self.inner.is_empty() {
            None
        } else {
            Some(&self.inner[self.inner.len() - 1])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Stack, Stackable};

    #[test]
    fn test_is_empty() {
        let mut stack = Stack::new();
        assert_eq!(stack.is_empty(), true);

        stack.push(1);
        assert_eq!(stack.is_empty(), false);
    }

    #[test]
    fn test_push_pop1() {
        let mut stack = Stack::new();
        stack.push(1);

        assert_eq!(stack.pop1(), Some(1));
        assert_eq!(stack.is_empty(), true);
    }

    #[test]
    fn test_pop() {
        let mut stack = Stack::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);
        stack.push(4);
        stack.push(5);
        stack.push(6);

        assert_eq!(stack.pop(1), Some(vec![6]));
        assert_eq!(stack.pop(2), Some(vec![4, 5]));
        assert_eq!(stack.pop(4), None); // not enough items
        assert_eq!(stack.pop(3), Some(vec![1, 2, 3]));
        assert_eq!(stack.pop1(), None);
        assert_eq!(stack.is_empty(), true);
    }

    #[test]
    fn test_peek1() {
        let mut stack = Stack::new();
        stack.push(1);
        stack.push(2);

        assert_eq!(stack.peek1(), Some(&2));
    }
}
