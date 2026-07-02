use proc_macro2::TokenStream;

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub module_name: String,
    pub indent: usize,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            module_name: "ts_aot_module".to_owned(),
            indent: 4,
        }
    }
}

#[must_use]
pub fn render_tokens(tokens: &TokenStream, _cfg: &RenderConfig) -> String {
    tokens.to_string()
}
