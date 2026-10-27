pub struct Counter {
    count: i64,
}

impl Counter {
    pub fn new() -> Self {
        Self { count: 0 }
    }

    pub fn value(&self) -> i64 {
        self.count
    }

    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn decrement(&mut self) {
        self.count -= 1;
    }
}
