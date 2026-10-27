// Ownership-heavy code: Box, Option<Box<T>>, references, mem::swap/take, Rc<RefCell<T>>.
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug)]
struct Node {
    val: i32,
    next: Option<Box<Node>>,
}

struct List {
    head: Option<Box<Node>>,
    len: usize,
}

impl List {
    fn new() -> Self {
        List { head: None, len: 0 }
    }
    fn push(&mut self, val: i32) {
        let old = self.head.take();
        self.head = Some(Box::new(Node { val, next: old }));
        self.len += 1;
    }
    fn pop(&mut self) -> Option<i32> {
        self.head.take().map(|n| {
            self.head = n.next;
            self.len -= 1;
            n.val
        })
    }
    fn reverse(&mut self) {
        let mut prev: Option<Box<Node>> = None;
        let mut cur = self.head.take();
        while let Some(mut node) = cur {
            cur = node.next.take();
            node.next = prev;
            prev = Some(node);
        }
        self.head = prev;
    }
    fn to_vec(&self) -> Vec<i32> {
        let mut out = Vec::new();
        let mut cur = &self.head;
        while let Some(node) = cur {
            out.push(node.val);
            cur = &node.next;
        }
        out
    }
    fn sum_recursive(node: &Option<Box<Node>>) -> i32 {
        match node {
            None => 0,
            Some(n) => n.val + List::sum_recursive(&n.next),
        }
    }
}

#[derive(Debug)]
enum Tree {
    Leaf,
    Node(Box<Tree>, i32, Box<Tree>),
}

impl Tree {
    fn insert(self, v: i32) -> Tree {
        match self {
            Tree::Leaf => Tree::Node(Box::new(Tree::Leaf), v, Box::new(Tree::Leaf)),
            Tree::Node(l, x, r) => {
                if v < x {
                    Tree::Node(Box::new(l.insert(v)), x, r)
                } else {
                    Tree::Node(l, x, Box::new(r.insert(v)))
                }
            }
        }
    }
    fn inorder(&self, out: &mut Vec<i32>) {
        if let Tree::Node(l, x, r) = self {
            l.inorder(out);
            out.push(*x);
            r.inorder(out);
        }
    }
    fn depth(&self) -> usize {
        match self {
            Tree::Leaf => 0,
            Tree::Node(l, _, r) => 1 + l.depth().max(r.depth()),
        }
    }
}

#[derive(Debug)]
struct Account {
    owner: String,
    balance: i64,
}

fn transfer(from: &mut Account, to: &mut Account, amount: i64) -> Result<(), String> {
    if from.balance < amount {
        return Err(format!("{} has insufficient funds", from.owner));
    }
    from.balance -= amount;
    to.balance += amount;
    Ok(())
}

fn bump_all(values: &mut [i32]) {
    for v in values.iter_mut() {
        *v += 1;
    }
}

fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

#[derive(Debug)]
struct Graph {
    edges: HashMap<u32, Vec<u32>>,
}

fn main() {
    let mut l = List::new();
    for i in 1..=5 {
        l.push(i);
    }
    println!("{:?} len={}", l.to_vec(), l.len);
    l.reverse();
    println!("{:?} pop={:?}", l.to_vec(), l.pop());
    println!("sum={}", List::sum_recursive(&l.head));

    let mut t = Tree::Leaf;
    for v in [5, 3, 8, 1, 4, 9, 7] {
        t = t.insert(v);
    }
    let mut out = Vec::new();
    t.inorder(&mut out);
    println!("{:?} depth={}", out, t.depth());

    let mut a = Account { owner: "alice".into(), balance: 100 };
    let mut b = Account { owner: "bob".into(), balance: 20 };
    println!("{:?}", transfer(&mut a, &mut b, 70));
    println!("{:?}", transfer(&mut a, &mut b, 70));
    println!("{:?} {:?}", a, b);
    std::mem::swap(&mut a, &mut b);
    println!("after swap: {} {}", a.owner, b.owner);
    let taken = std::mem::take(&mut a.owner);
    println!("taken={:?} left={:?}", taken, a.owner);
    let old = std::mem::replace(&mut b.balance, 0);
    println!("old={} new={}", old, b.balance);

    let mut nums = vec![1, 2, 3];
    bump_all(&mut nums);
    bump_all(&mut nums[1..]);
    println!("{:?}", nums);
    let r = &mut nums;
    r.push(10);
    let first = &mut r[0];
    *first *= 100;
    println!("{:?}", nums);
    println!("{}", longest("hello", "hi"));

    let mut x = 5;
    {
        let y = &mut x;
        *y += 1;
    }
    let z = &x;
    println!("x={} z={}", x, *z + 1);

    let shared = Rc::new(RefCell::new(vec![1, 2]));
    let other = Rc::clone(&shared);
    other.borrow_mut().push(3);
    shared.borrow_mut().push(4);
    println!("{:?} rc={}", shared.borrow(), Rc::strong_count(&shared));
    let counter = Rc::new(RefCell::new(0));
    let handles: Vec<Rc<RefCell<i32>>> = (0..3).map(|_| Rc::clone(&counter)).collect();
    for h in &handles {
        *h.borrow_mut() += 10;
    }
    println!("counter={}", counter.borrow());

    let mut g = Graph { edges: HashMap::new() };
    g.edges.entry(1).or_default().push(2);
    g.edges.entry(1).or_default().push(3);
    g.edges.entry(2).or_default().push(3);
    let mut keys: Vec<_> = g.edges.keys().collect();
    keys.sort();
    for k in keys {
        println!("{} -> {:?}", k, g.edges[k]);
    }

    let mut pair = (1, String::from("a"));
    let (ref mut n, ref s) = pair;
    *n += 1;
    println!("{} {}", n, s);
    pair.1.push('b');
    println!("{:?}", pair);

    let mut opt = Some(3);
    if let Some(v) = opt.as_mut() {
        *v *= 7;
    }
    if let Some(ref mut v) = opt {
        *v += 1;
    }
    println!("{:?}", opt);
    let mut grid = [[0u8; 3]; 3];
    for (i, row) in grid.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (i * 3 + j) as u8;
        }
    }
    let copy = grid;
    grid[0][0] = 99;
    println!("{:?} {:?}", grid[0], copy[0]);
}
