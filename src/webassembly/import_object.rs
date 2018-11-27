//! The webassembly::ImportObject is an structure containing the values to be
//! imported into the newly-created webassembly::Instance, such as functions
//! or webassembly::Memory objects.
// Code inspired from: https://stackoverflow.com/a/45795699/1072990
// Adapted to the Webassembly use case
use crate::webassembly::LinearMemory;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

// We introduced the Pair and BorrowedPair types. We can't use (A, B)
// directly due to the orphan rule E0210. This is fine since the map
// is an implementation detail.
#[derive(PartialEq, Eq, Hash)]
pub struct Pair<A, B>(pub A, pub B);

#[derive(PartialEq, Eq, Hash)]
struct BorrowedPair<'a, 'b, A: 'a, B: 'b>(&'a A, &'b B);

// The KeyPair trait takes the role of Q we mentioned above. We'd need
// to impl Eq + Hash for KeyPair, but Eq and Hash are both not object
// safe. We add the a() and b() methods to help implementing them manually.
trait KeyPair<A, B> {
    /// Obtains the first element of the pair.
    fn a(&self) -> &A;
    /// Obtains the second element of the pair.
    fn b(&self) -> &B;
}

// Now we implement the Borrow trait from Pair<A, B> to KeyPair + 'a.
// Note the 'a — this is a subtle bit that is needed to make
// Table::get actually work. The arbitrary 'a allows us to say that a
// Pair<A, B> can be borrowed to the trait object for any lifetime.
// If we don't specify the  'a, the unsized trait object will default
// to 'static, meaning the Borrow trait can only be applied when the
// implementation like BorrowedPair outlives 'static, which is certainly
// not the case.
impl<'a, A, B> Borrow<KeyPair<A, B> + 'a> for Pair<A, B>
where
    A: Eq + Hash + 'a,
    B: Eq + Hash + 'a,
{
    fn borrow(&self) -> &(KeyPair<A, B> + 'a) {
        self
    }
}

// Finally, we implement Eq and Hash. As above, we implement for KeyPair + 'a
// instead of KeyPair (which means KeyPair + 'static in this context).
impl<'a, A: Hash, B: Hash> Hash for (KeyPair<A, B> + 'a) {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.a().hash(state);
        self.b().hash(state);
    }
}

impl<'a, A: Eq, B: Eq> PartialEq for (KeyPair<A, B> + 'a) {
    fn eq(&self, other: &Self) -> bool {
        self.a() == other.a() && self.b() == other.b()
    }
}

impl<'a, A: Eq, B: Eq> Eq for (KeyPair<A, B> + 'a) {}

// OP's ImportObject struct
pub struct ImportObject<A: Eq + Hash, B: Eq + Hash> {
    pub map: HashMap<Pair<A, B>, ImportValue>,
}

impl<A: Eq + Hash, B: Eq + Hash> ImportObject<A, B> {
    pub fn new() -> Self {
        ImportObject {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, a: &A, b: &B) -> Option<&ImportValue> {
        self.map.get(&BorrowedPair(a, b) as &KeyPair<A, B>)
    }

    pub fn set(&mut self, a: A, b: B, v: ImportValue) {
        self.map.insert(Pair(a, b), v);
    }
}

impl<A, B> KeyPair<A, B> for Pair<A, B>
where
    A: Eq + Hash,
    B: Eq + Hash,
{
    fn a(&self) -> &A {
        &self.0
    }
    fn b(&self) -> &B {
        &self.1
    }
}
impl<'a, 'b, A, B> KeyPair<A, B> for BorrowedPair<'a, 'b, A, B>
where
    A: Eq + Hash + 'a,
    B: Eq + Hash + 'b,
{
    fn a(&self) -> &A {
        self.0
    }
    fn b(&self) -> &B {
        self.1
    }
}

#[derive(PartialEq, Debug)]
pub enum ImportValue {
    Func(*const u8),
    Global(i64),
    Table(Vec<usize>),
    Memory(LinearMemory),
}

#[cfg(test)]
mod tests {
    use super::ImportObject;
    use super::ImportValue;

    #[test]
    fn test_import_object() {
        fn x() {}
        let mut import_object = ImportObject::new();
        import_object.set("abc", "def", ImportValue::Func(x as _));
        assert_eq!(
            *import_object.get(&"abc", &"def").unwrap(),
            ImportValue::Func(x as _)
        );
    }
}
