//! Template renderer for manifest commands and health checks.
//!
//! Supports `{{workingDir}}` and `{{secret.NAME}}` substitutions.
//! Filled in by Wave 2 subagent.

use std::collections::HashMap;

/// Context for template rendering.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    pub working_dir: String,
    pub secrets: HashMap<String, String>,
}

/// Render a template string by substituting `{{key}}` placeholders.
///
/// Supported placeholders:
/// - `{{workingDir}}` — replaced with the manifest's working directory
/// - `{{secret.NAME}}` — replaced with the value of secret NAME
///
/// Unknown placeholders are left as-is (no error) to allow shell variables
/// like `$HOME` to pass through.
pub fn render_template(template: &str, ctx: &TemplateContext) -> String {
    let mut result = template.to_string();
    result = result.replace("{{workingDir}}", &ctx.working_dir);
    for (name, value) in &ctx.secrets {
        let placeholder = format!("{{{{secret.{}}}}}", name);
        result = result.replace(&placeholder, value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_working_dir() {
        let ctx = TemplateContext {
            working_dir: "/workspaces/DataMigrata".into(),
            ..Default::default()
        };
        let result = render_template("cd {{workingDir}} && ls", &ctx);
        assert_eq!(result, "cd /workspaces/DataMigrata && ls");
    }

    #[test]
    fn test_render_secret() {
        let mut secrets = HashMap::new();
        secrets.insert("SA_PASSWORD".into(), "s3cr3t".into());
        let ctx = TemplateContext {
            working_dir: "/x".into(),
            secrets,
        };
        let result = render_template("echo {{secret.SA_PASSWORD}}", &ctx);
        assert_eq!(result, "echo s3cr3t");
    }

    #[test]
    fn test_unknown_placeholder_passes_through() {
        let ctx = TemplateContext::default();
        let result = render_template("echo $HOME {{unknown}}", &ctx);
        assert_eq!(result, "echo $HOME {{unknown}}");
    }
}
