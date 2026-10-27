pub fn score(text: &str) -> usize {
    text.len()
}
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
pub fn identity<T>(value: T) -> T {
    value
}
pub fn doubled(values: Vec<i32>) -> Vec<i32> {
    values.into_iter().map(|v| v * 2).collect()
}
