use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

// See explanation (1).
#[derive(PartialEq, Eq, Hash)]
struct Pair<A, B>(A, B);

#[derive(PartialEq, Eq, Hash)]
struct BorrowedPair<'a, 'b, A: 'a, B: 'b>(&'a A, &'b B);

// See explanation (2).
trait KeyPair<A, B> {
    /// Obtains the first element of the pair.
    fn a(&self) -> &A;
    /// Obtains the second element of the pair.
    fn b(&self) -> &B;
}

// See explanation (3).
impl<'a, A, B> Borrow<KeyPair<A, B> + 'a> for Pair<A, B>
where
    A: Eq + Hash + 'a,
    B: Eq + Hash + 'a,
{
    fn borrow(&self) -> &(KeyPair<A, B> + 'a) {
        self
    }
}

// See explanation (4).
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
    map: HashMap<Pair<A, B>, *const u8>,
}

impl<A: Eq + Hash, B: Eq + Hash> ImportObject<A, B> {
    pub fn new() -> Self {
        ImportObject {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, a: &A, b: &B) -> Option<*const u8> {
        self.map
            .get(&BorrowedPair(a, b) as &KeyPair<A, B>)
            .map(|p| *p)
    }

    pub fn set(&mut self, a: A, b: B, v: *const u8) {
        self.map.insert(Pair(a, b), v);
    }
}

// pub struct ImportObject<A: Eq + Hash, B: Eq + Hash> {
//     map: HashMap<Pair<A, B>, *const u8>,
// }

// impl<A: Eq + Hash, B: Eq + Hash> ImportObject<A, B> {
//     pub fn new() -> Self {
//         ImportObject { map: HashMap::new() }
//     }

//     pub fn get(&self, a: &A, b: &B) -> *const u8 {
//         *self.map.get(&BorrowedPair(a, b) as &KeyPair<A, B>).unwrap()
//     }

//     pub fn set(&mut self, a: A, b: B, v: *const u8) {
//         self.map.insert(Pair(a, b), v);
//     }
// }
// Boring stuff below.

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

//----------------------------------------------------------------

// #[derive(Eq, PartialEq, Hash)]
// struct A(&'static str);

// #[derive(Eq, PartialEq, Hash)]
// struct B(&'static str);

#[cfg(test)]
mod tests {
    use super::ImportObject;

    #[test]
    fn test_import_object() {
        fn x() {}
        let mut import_object = ImportObject::new();
        import_object.set("abc", "def", x as *const u8);
        // import_object.set("123"), A("456"), 45.0);
        assert_eq!(import_object.get(&"abc", &"def").unwrap(), x as *const u8);
        // assert_eq!(import_object.get(&"abc", &"dxf"), 4.0);
        // assert_eq!(import_object.get(&A("123"), &A("456")), 45.0);
    }
}
