#[link(wasm_import_module = "wasmer_suspend")]
extern "C" {
    fn suspend();
}

use std::collections::BTreeMap;

#[derive(Default)]
struct Node {
    count: usize,
    children: BTreeMap<char, Node>,
}

impl Node {
    fn insert(&mut self, mut s: impl Iterator<Item = char>) {
        match s.next() {
            Some(x) => {
                self.children.entry(x).or_default().insert(s);
            }
            None => {
                self.count += 1;
            }
        }
    }

    fn for_each_dyn(&self, cb: &dyn Fn(&str, usize), prefix: &mut String) {
        if self.count > 0 {
            cb(&prefix, self.count);
        }

        for (k, v) in self.children.iter() {
            prefix.push(*k);
            v.for_each_dyn(cb, prefix);
            prefix.pop().unwrap();
        }
    }
}

fn main() {
    let mut root = Node::default();
    root.insert("Ava".chars());
    root.insert("Alexander".chars());
    root.insert("Aiden".chars());
    root.insert("Bella".chars());
    root.insert("Brianna".chars());
    root.insert("Brielle".chars());
    root.insert("Charlotte".chars());
    root.insert("Chloe".chars());
    root.insert("Camila".chars());

    println!("Tree ready, suspending.");
    unsafe {
        suspend();
    }

    root.for_each_dyn(
        &|seq, count| {
            println!("{}: {}", seq, count);
            unsafe {
                suspend();
            }
        },
        &mut "".into(),
    );

    println!("[END]");
}
