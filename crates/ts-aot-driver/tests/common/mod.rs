pub fn normalize_rust(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .replace(" :: ", "::")
        .replace(" <", "<")
        .replace("> ", ">")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
}
