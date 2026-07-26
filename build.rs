fn main() {
    // Compile the declarative Slint interface into Rust bindings.
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
}
