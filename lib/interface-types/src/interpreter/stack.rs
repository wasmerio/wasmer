pub trait Stackable {
    type Item;

    fn is_empty(&self) -> bool;
    fn as_slice(&self) -> &[Self::Item];
    fn push(&mut self, item: Self::Item);
    fn pop1(&mut self) -> Option<Self::Item>;
    fn pop(&mut self, n: usize) -> Option<Vec<Self::Item>>;
}

#[derive(Debug, Default)]
pub struct Stack<T>
where
    T: Default + Clone,
{
    inner: Vec<T>,
}

impl<T> Stack<T>
where
    T: Default + Clone,
{
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
                .rev()
                .collect::<Vec<Self::Item>>();

            assert!(items.len() == n);

            Some(items)
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
        assert_eq!(stack.pop(2), Some(vec![5, 4]));
        assert_eq!(stack.pop(4), None); // not enough items
        assert_eq!(stack.pop(3), Some(vec![3, 2, 1]));
        assert_eq!(stack.pop1(), None);
        assert_eq!(stack.is_empty(), true);
    }
}
