use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        SelectorStyle, SelectorType, check_selector_prefix, check_selector_style,
        extract_selector_name, get_component_metadata,
        get_decorator_name, get_metadata_property, parse_selector_type
    },
};

fn directive_selector_type_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Directive selector should be used as {expected}"))
        .with_help(format!(
            "Change the selector to use {expected} style (e.g., '[appExample]' for attribute)"
        ))
        .with_label(span)
}

fn directive_selector_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    let prefix_list = prefixes.join(", ");
    OxcDiagnostic::warn(format!("Directive selector should be prefixed with one of: {prefix_list}"))
        .with_help("Add a prefix to the selector (e.g., '[appHighlight]' with prefix 'app')")
        .with_label(span)
}

fn directive_selector_style_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Directive selector should be {expected}"))
        .with_help(format!("Use {expected} for the selector (e.g., 'appHighlight' for camelCase)"))
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct DirectiveSelectorConfig {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    prefix: PrefixConfig,
    #[serde(default = "default_style")]
    style: String
}

#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(untagged)]
pub enum PrefixConfig {
    Single(String),
    Multiple(Vec<String>),
    #[default]
    None
}

impl PrefixConfig {
    fn as_vec(&self) -> Vec<String> {
        match self {
            PrefixConfig::Single(s) => vec![s.clone()],
            PrefixConfig::Multiple(v) => v.clone(),
            PrefixConfig::None => vec![]
}
    }
}

fn default_style() -> String {
    "camelCase".to_string()
}

impl Default for DirectiveSelectorConfig {
    fn default() -> Self {
        Self {
            r#type: Some("attribute".to_string()),
            prefix: PrefixConfig::None,
            style: default_style()
}
    }
}

#[derive(Debug, Clone)]
pub struct DirectiveSelector {
    selector_type: Option<SelectorType>,
    prefixes: Vec<String>,
    style: SelectorStyle
}

impl Default for DirectiveSelector {
    fn default() -> Self {
        Self {
            selector_type: Some(SelectorType::Attribute),
            prefixes: vec![],
            style: SelectorStyle::CamelCase
}
    }
}

impl From<DirectiveSelectorConfig> for DirectiveSelector {
    fn from(config: DirectiveSelectorConfig) -> Self {
        let selector_type = config.r#type.as_deref().and_then(|t| match t {
            "element" => Some(SelectorType::Element),
            "attribute" => Some(SelectorType::Attribute),
            _ => None
});
        let style = match config.style.as_str() {
            "kebab-case" => SelectorStyle::KebabCase,
            _ => SelectorStyle::CamelCase
};
        Self { selector_type, prefixes: config.prefix.as_vec(), style }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Validates Angular directive selectors against configured type, prefix, and style rules.
    ///
    /// ### Why is this bad?
    ///
    /// Consistent selector conventions help:
    /// - Avoid naming collisions with native HTML attributes or third-party directives
    /// - Easily identify your application's directives
    /// - Maintain consistency across the codebase
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/directive-selector": ["error", {
    ///     "type": "attribute",
    ///     "prefix": "app",
    ///     "style": "camelCase"
    ///   }]
    /// }
    /// ```
    ///
    /// - `type`: "element" or "attribute"
    /// - `prefix`: string or array of strings
    /// - `style`: "kebab-case" or "camelCase"
    ///
    /// ### Examples
    ///
    /// With configuration `{ "type": "attribute", "prefix": "app", "style": "camelCase" }`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// @Directive({ selector: '[highlight]' })  // Missing prefix
    /// @Directive({ selector: '[app-highlight]' })  // Wrong style
    /// @Directive({ selector: 'app-highlight' })  // Wrong type
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// @Directive({ selector: '[appHighlight]' })
    /// ```
    DirectiveSelector,
    angular,
    pedantic,
    pending,
    config = DirectiveSelectorConfig
);

impl Rule for DirectiveSelector {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        serde_json::from_value::<DirectiveSelectorConfig>(config_value.clone()).map(Into::into)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Directive decorator
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Directive" {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Get the selector property value expression (for accurate span reporting)
        let Some(selector_expr) = get_metadata_property(metadata, "selector") else {
            return;
        };

        // Extract the string value from the selector expression
        let selector = match selector_expr {
            oxc_ast::ast::Expression::StringLiteral(lit) => lit.value.as_str(),
            oxc_ast::ast::Expression::TemplateLiteral(lit) => {
                if lit.expressions.is_empty() && lit.quasis.len() == 1 {
                    lit.quasis[0].value.raw.as_str()
                } else {
                    return;
                }
            }
            _ => return,
        };

        // Extract the selector name
        let Some(selector_name) = extract_selector_name(selector) else {
            return;
        };

        // Get the span for error reporting (use selector value span, matching ESLint)
        let selector_span = selector_expr.span();

        // Check type
        if let Some(expected_type) = &self.selector_type
            && let Some(actual_type) = parse_selector_type(selector)
                && actual_type != *expected_type {
                    let type_str = match expected_type {
                        SelectorType::Element => "an element",
                        SelectorType::Attribute => "an attribute",
                    };
                    ctx.diagnostic(directive_selector_type_diagnostic(selector_span, type_str));
                    return;
                }

        // Check prefix
        if !self.prefixes.is_empty() {
            let prefix_refs: Vec<&str> = self.prefixes.iter().map(std::string::String::as_str).collect();
            if !check_selector_prefix(selector_name, &prefix_refs) {
                ctx.diagnostic(directive_selector_prefix_diagnostic(
                    selector_span,
                    &self.prefixes,
                ));
                return;
            }
        }

        // Check style
        if !check_selector_style(selector_name, self.style) {
            let style_str = match self.style {
                SelectorStyle::KebabCase => "kebab-case",
                SelectorStyle::CamelCase => "camelCase",
            };
            ctx.diagnostic(directive_selector_style_diagnostic(selector_span, style_str));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Valid camelCase attribute with prefix
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: '[appHighlight]'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
        // Multiple allowed prefixes
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: '[myHighlight]'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "my"], "style": "camelCase" }]),
            ),
        ),
        // No prefix requirement
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: '[highlight]'
            })
            class HighlightDirective {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "style": "camelCase" }])),
        ),
        // Non-Angular Directive
        (
            r"
            import { Directive } from 'other-lib';
            @Directive({
                selector: 'INVALID'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
    ];

    let fail = vec![
        // Wrong type (element instead of attribute)
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: 'app-highlight'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
        // Missing prefix
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: '[highlight]'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
        // Wrong style (kebab-case instead of camelCase)
        (
            r"
            import { Directive } from '@angular/core';
            @Directive({
                selector: '[app-highlight]'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
    ];

    Tester::new(DirectiveSelector::NAME, DirectiveSelector::PLUGIN, pass, fail).test_and_snapshot();
}
