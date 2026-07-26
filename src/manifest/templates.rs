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

    // -------------------- additional tests --------------------

    #[test]
    fn test_render_no_placeholders_returns_input_unchanged() {
        let ctx = TemplateContext {
            working_dir: "/workspaces".into(),
            ..Default::default()
        };
        let result = render_template("echo hello world", &ctx);
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_render_multiple_working_dir_substitutions() {
        let ctx = TemplateContext {
            working_dir: "/workspaces/app".into(),
            ..Default::default()
        };
        let result = render_template(
            "cd {{workingDir}} && pwd && ls {{workingDir}} && rm -rf {{workingDir}}/tmp",
            &ctx,
        );
        assert_eq!(
            result,
            "cd /workspaces/app && pwd && ls /workspaces/app && rm -rf /workspaces/app/tmp"
        );
    }

    #[test]
    fn test_render_secret_substitutes_value() {
        let mut secrets = HashMap::new();
        secrets.insert("API_KEY".into(), "abc123".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template("export API_KEY={{secret.API_KEY}}", &ctx);
        assert_eq!(result, "export API_KEY=abc123");
    }

    #[test]
    fn test_render_multiple_different_secrets() {
        let mut secrets = HashMap::new();
        secrets.insert("USER".into(), "admin".into());
        secrets.insert("PASS".into(), "p@ssw0rd".into());
        secrets.insert("DB".into(), "mydb".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template(
            "psql -U {{secret.USER}} -d {{secret.DB}} -W {{secret.PASS}}",
            &ctx,
        );
        assert_eq!(result, "psql -U admin -d mydb -W p@ssw0rd");
    }

    #[test]
    fn test_render_unknown_secret_placeholder_left_as_is() {
        let ctx = TemplateContext {
            working_dir: "/".into(),
            ..Default::default()
        };
        let result = render_template("echo {{secret.NOT_THERE}}", &ctx);
        assert_eq!(result, "echo {{secret.NOT_THERE}}");
    }

    #[test]
    fn test_render_mixed_known_and_unknown_placeholders() {
        let mut secrets = HashMap::new();
        secrets.insert("KNOWN".into(), "v1".into());
        let ctx = TemplateContext {
            working_dir: "/work".into(),
            secrets,
        };
        let result = render_template(
            "cd {{workingDir}} && export K={{secret.KNOWN}} U={{secret.UNKNOWN}} {{unknown}}",
            &ctx,
        );
        assert_eq!(
            result,
            "cd /work && export K=v1 U={{secret.UNKNOWN}} {{unknown}}"
        );
    }

    #[test]
    fn test_render_empty_template_returns_empty() {
        let ctx = TemplateContext {
            working_dir: "/".into(),
            ..Default::default()
        };
        let result = render_template("", &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_empty_context_does_not_substitute() {
        let ctx = TemplateContext::default();
        // working_dir defaults to "" so {{workingDir}} → ""
        let result = render_template("cd {{workingDir}}", &ctx);
        assert_eq!(result, "cd ");
    }

    #[test]
    fn test_render_empty_context_unknown_placeholder_unchanged() {
        let ctx = TemplateContext::default();
        let result = render_template("{{unknown}} {{secret.FOO}}", &ctx);
        assert_eq!(result, "{{unknown}} {{secret.FOO}}");
    }

    #[test]
    fn test_render_malformed_placeholder_does_not_panic() {
        let ctx = TemplateContext {
            working_dir: "/x".into(),
            ..Default::default()
        };
        // Unterminated placeholder — render_template must not panic.
        let result = render_template("echo {{workingDir", &ctx);
        // The un-closed placeholder is left as-is since it doesn't match
        // {{workingDir}} exactly.
        assert_eq!(result, "echo {{workingDir");
    }

    #[test]
    fn test_render_malformed_secret_placeholder_does_not_panic() {
        let mut secrets = HashMap::new();
        secrets.insert("FOO".into(), "bar".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        // Unterminated secret placeholder — should not match {{secret.FOO}}
        // so it is left unchanged.
        let result = render_template("echo {{secret.FOO", &ctx);
        assert_eq!(result, "echo {{secret.FOO");
    }

    #[test]
    fn test_render_secret_with_underscore_name() {
        let mut secrets = HashMap::new();
        secrets.insert("MY_SECRET".into(), "val_123".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template("echo {{secret.MY_SECRET}}", &ctx);
        assert_eq!(result, "echo val_123");
    }

    #[test]
    fn test_render_secret_with_numbers_in_name() {
        let mut secrets = HashMap::new();
        secrets.insert("SECRET_123".into(), "numeric_name_val".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template("echo {{secret.SECRET_123}}", &ctx);
        assert_eq!(result, "echo numeric_name_val");
    }

    #[test]
    fn test_render_secret_value_containing_braces() {
        // The substituted value itself contains `{{...}}` — the renderer must
        // not re-process it.
        let mut secrets = HashMap::new();
        secrets.insert("WEIRD".into(), "{{workingDir}}".into());
        let ctx = TemplateContext {
            working_dir: "/real".into(),
            secrets,
        };
        let result = render_template("{{secret.WEIRD}}", &ctx);
        // The first pass substitutes the secret value, but since render uses
        // `str::replace`, it does NOT re-scan the output for new placeholders.
        // The user-supplied secret value is emitted verbatim.
        assert_eq!(result, "{{workingDir}}");
    }

    #[test]
    fn test_render_secret_value_empty_string() {
        let mut secrets = HashMap::new();
        secrets.insert("EMPTY".into(), "".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template("[{{secret.EMPTY}}]", &ctx);
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_render_secret_name_case_sensitive() {
        let mut secrets = HashMap::new();
        secrets.insert("Mixed_Case".into(), "v1".into());
        let ctx = TemplateContext {
            working_dir: "/".into(),
            secrets,
        };
        let result = render_template("{{secret.Mixed_Case}}", &ctx);
        assert_eq!(result, "v1");
        // Wrong case should not match.
        let result2 = render_template("{{secret.mixed_case}}", &ctx);
        assert_eq!(result2, "{{secret.mixed_case}}");
    }

    #[test]
    fn test_render_working_dir_with_special_chars() {
        let ctx = TemplateContext {
            working_dir: "/workspaces/my app/with spaces".into(),
            ..Default::default()
        };
        let result = render_template("cd \"{{workingDir}}\"", &ctx);
        assert_eq!(result, "cd \"/workspaces/my app/with spaces\"");
    }
}
