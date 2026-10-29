fn main() {
    println!("tool.location=rust-prototype");
    println!(
        "tool.args={}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );
}
