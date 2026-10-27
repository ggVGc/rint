// Structs, traits, generics, dynamic dispatch, operator overloading.
use std::fmt;
use std::ops::{Add, Mul};

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> String {
        String::from("shape")
    }
    fn describe(&self) -> String {
        format!("{} with area {:.2}", self.name(), self.area())
    }
}

struct Circle {
    r: f64,
}
struct Square(f64);

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
    fn name(&self) -> String {
        "circle".to_string()
    }
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.0 * self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct V2 {
    x: f64,
    y: f64,
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2 { x: self.x + o.x, y: self.y + o.y }
    }
}

impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, k: f64) -> V2 {
        V2 { x: self.x * k, y: self.y * k }
    }
}

impl fmt::Display for V2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}, {}>", self.x, self.y)
    }
}

fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

fn largest<T: PartialOrd + Copy>(items: &[T]) -> T {
    let mut best = items[0];
    for &it in items {
        if it > best {
            best = it;
        }
    }
    best
}

struct Counter {
    n: u32,
}

impl Iterator for Counter {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.n < 5 {
            self.n += 1;
            Some(self.n)
        } else {
            None
        }
    }
}

trait Animal {
    fn speak(&self) -> String;
}
struct Dog;
struct Cat {
    lives: u8,
}
impl Animal for Dog {
    fn speak(&self) -> String {
        "Woof".into()
    }
}
impl Animal for Cat {
    fn speak(&self) -> String {
        format!("Meow x{}", self.lives)
    }
}

fn main() {
    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Circle { r: 1.0 }), Box::new(Square(2.0))];
    for s in &shapes {
        println!("{}", s.describe());
    }
    println!("total {:.4}", total_area(&shapes));

    let a = V2 { x: 1.0, y: 2.0 };
    let b = V2 { x: 0.5, y: -1.0 };
    let c = a + b * 2.0;
    println!("{} {:?} {}", c, c, a == V2 { x: 1.0, y: 2.0 });
    println!("{:?}", V2::default());

    println!("{} {} {}", largest(&[3, 9, 2]), largest(&[1.5, 0.2]), largest(&['x', 'b']));

    let total: u32 = Counter { n: 0 }.zip(Counter { n: 0 }.skip(1)).map(|(a, b)| a * b).filter(|x| x % 3 == 0).sum();
    println!("counter total {}", total);
    let mut c = Counter { n: 2 };
    println!("{:?} {:?}", c.next(), c.collect::<Vec<_>>());

    let animals: Vec<Box<dyn Animal>> = vec![Box::new(Dog), Box::new(Cat { lives: 9 })];
    let sounds: Vec<String> = animals.iter().map(|a| a.speak()).collect();
    println!("{}", sounds.join(", "));
}
