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
        SelectorStyle, SelectorType, get_component_metadata,
        get_decorator_name, get_metadata_property,
    },
};

fn directive_selector_type_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should be used as an {expected} (https://angular.dev/style-guide#style-02-06)"))
        .with_label(span)
}

fn directive_selector_prefix_diagnostic(span: Span, prefix_text: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should start with one of these prefixes: {prefix_text} (https://angular.dev/style-guide#style-02-08)"))
        .with_label(span)
}

fn directive_selector_style_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should be {expected} (https://angular.dev/style-guide#style-02-06)"))
        .with_label(span)
}

fn directive_selector_after_prefix_diagnostic(span: Span, prefix_text: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("There should be a selector after the {prefix_text} prefix"))
        .with_label(span)
}

/// Configuration for a single selector rule.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SingleSelectorConfig {
    #[serde(default)]
    r#type: TypeConfig,
    #[serde(default)]
    prefix: PrefixConfig,
    #[serde(default = "default_style")]
    style: String,
}

/// Type can be a single string or array of strings.
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(untagged)]
pub enum TypeConfig {
    Single(String),
    Multiple(Vec<String>),
    #[default]
    None,
}

impl TypeConfig {
    fn as_vec(&self) -> Vec<String> {
        match self {
            TypeConfig::Single(s) => vec![s.clone()],
            TypeConfig::Multiple(v) => v.clone(),
            TypeConfig::None => vec![],
        }
    }
}

/// Prefix can be a single string or array of strings.
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(untagged)]
pub enum PrefixConfig {
    Single(String),
    Multiple(Vec<String>),
    #[default]
    None,
}

impl PrefixConfig {
    fn as_vec(&self) -> Vec<String> {
        match self {
            PrefixConfig::Single(s) => {
                if s.is_empty() {
                    vec![]
                } else {
                    vec![s.clone()]
                }
            }
            PrefixConfig::Multiple(v) => v.iter().filter(|s| !s.is_empty()).cloned().collect(),
            PrefixConfig::None => vec![],
        }
    }
}

fn default_style() -> String {
    "camelCase".to_string()
}

impl Default for SingleSelectorConfig {
    fn default() -> Self {
        Self {
            r#type: TypeConfig::Single("attribute".to_string()),
            prefix: PrefixConfig::None,
            style: default_style(),
        }
    }
}

/// Full configuration - can be a single config object or array of config objects.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DirectiveSelectorConfig {
    Single(SingleSelectorConfig),
    Multiple(Vec<SingleSelectorConfig>),
}

impl Default for DirectiveSelectorConfig {
    fn default() -> Self {
        DirectiveSelectorConfig::Single(SingleSelectorConfig::default())
    }
}

/// A single parsed rule configuration.
#[derive(Debug, Clone)]
pub struct ParsedSelectorRule {
    selector_types: Vec<SelectorType>,
    prefixes: Vec<String>,
    style: SelectorStyle,
}

impl From<SingleSelectorConfig> for ParsedSelectorRule {
    fn from(config: SingleSelectorConfig) -> Self {
        let selector_types: Vec<SelectorType> = config
            .r#type
            .as_vec()
            .iter()
            .filter_map(|t| match t.as_str() {
                "element" => Some(SelectorType::Element),
                "attribute" => Some(SelectorType::Attribute),
                _ => None,
            })
            .collect();
        let style = match config.style.as_str() {
            "kebab-case" => SelectorStyle::KebabCase,
            _ => SelectorStyle::CamelCase,
        };
        Self {
            selector_types,
            prefixes: config.prefix.as_vec(),
            style,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectiveSelector {
    rules: Vec<ParsedSelectorRule>,
}

impl Default for DirectiveSelector {
    fn default() -> Self {
        Self {
            rules: vec![ParsedSelectorRule {
                selector_types: vec![SelectorType::Attribute],
                prefixes: vec![],
                style: SelectorStyle::CamelCase,
            }],
        }
    }
}

impl From<DirectiveSelectorConfig> for DirectiveSelector {
    fn from(config: DirectiveSelectorConfig) -> Self {
        let rules = match config {
            DirectiveSelectorConfig::Single(single) => vec![ParsedSelectorRule::from(single)],
            DirectiveSelectorConfig::Multiple(multiple) => {
                multiple.into_iter().map(ParsedSelectorRule::from).collect()
            }
        };
        Self { rules }
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
        let selector_raw = match selector_expr {
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

        // Get the span for error reporting (use selector value span, matching ESLint)
        let selector_span = selector_expr.span();

        // Parse the selector to extract valid selectors for each type
        let parsed = parse_css_selector(selector_raw);
        if parsed.elements.is_empty() && parsed.attributes.is_empty() {
            return;
        }

        // Check if any rule matches completely
        for rule in &self.rules {
            // Match ESLint's checkValidOptions behavior:
            // - type must have at least one valid value (element or attribute)
            // - prefix is optional (can be empty/missing)
            // - style is always valid since we default to camelCase
            if rule.selector_types.is_empty() {
                // Invalid rule configuration - skip entirely
                continue;
            }

            // Get selectors matching this rule's type(s)
            let valid_selectors = get_valid_selectors_for_types(&parsed, &rule.selector_types);

            if valid_selectors.is_empty() {
                // Type doesn't match, continue to check if we should report type error
                continue;
            }

            // Check prefix with style awareness
            let prefix_ok = rule.prefixes.is_empty()
                || valid_selectors.iter().any(|sel| {
                    check_prefix_with_style(sel, &rule.prefixes, rule.style)
                });

            if !prefix_ok {
                continue;
            }

            // Check if there's content after prefix (selectorAfterPrefix check)
            let has_content_after_prefix = rule.prefixes.is_empty()
                || valid_selectors.iter().any(|sel| {
                    has_selector_after_prefix(sel, &rule.prefixes, rule.style)
                });

            if !has_content_after_prefix {
                continue;
            }

            // Check style
            let style_ok = valid_selectors.iter().any(|sel| check_style(sel, rule.style));

            if style_ok {
                // This rule matches completely, no error
                return;
            }
        }

        // No rule matched completely, find the best error to report
        // Find a rule that matches by type to report the most specific error
        for rule in &self.rules {
            // Skip invalid rules (same check as above)
            if rule.selector_types.is_empty() {
                continue;
            }

            let valid_selectors = get_valid_selectors_for_types(&parsed, &rule.selector_types);

            if valid_selectors.is_empty() {
                // Report type error (priority 1)
                let type_str = if rule.selector_types.contains(&SelectorType::Attribute) {
                    "attribute"
                } else {
                    "element"
                };
                ctx.diagnostic(directive_selector_type_diagnostic(selector_span, type_str));
                return;
            }

            // Check content after prefix (priority 2 - only if prefix is required)
            if !rule.prefixes.is_empty() {
                let has_content_after_prefix = valid_selectors.iter().any(|sel| {
                    has_selector_after_prefix(sel, &rule.prefixes, rule.style)
                });

                if !has_content_after_prefix {
                    let prefix_text = format_prefix_for_message(&rule.prefixes);
                    ctx.diagnostic(directive_selector_after_prefix_diagnostic(
                        selector_span,
                        &prefix_text,
                    ));
                    return;
                }
            }

            // Check style (priority 3)
            let style_ok = valid_selectors.iter().any(|sel| check_style(sel, rule.style));

            if !style_ok {
                let style_str = match rule.style {
                    SelectorStyle::KebabCase => "kebab-case",
                    SelectorStyle::CamelCase => "camelCase",
                };
                ctx.diagnostic(directive_selector_style_diagnostic(selector_span, style_str));
                return;
            }

            // Check prefix (priority 4 - only if prefix is required)
            if !rule.prefixes.is_empty() {
                let prefix_ok = valid_selectors.iter().any(|sel| {
                    check_prefix_with_style(sel, &rule.prefixes, rule.style)
                });

                if !prefix_ok {
                    let prefix_text = format_prefix_list(&rule.prefixes);
                    ctx.diagnostic(directive_selector_prefix_diagnostic(selector_span, &prefix_text));
                    return;
                }
            }

            // All checks passed for this rule but we still got here - should not happen
            // This might occur if the first pass found a partial match but we're in the error pass
        }
    }
}

/// Parsed CSS selector with extracted element and attribute names.
#[derive(Debug, Default)]
struct ParsedSelector {
    elements: Vec<String>,
    attributes: Vec<String>,
}

/// Parse a CSS selector string to extract element and attribute names.
/// Handles complex selectors like "app-foo[bar].class" and comma-separated lists.
fn parse_css_selector(selector: &str) -> ParsedSelector {
    let mut result = ParsedSelector::default();

    // Handle comma-separated selectors
    for part in selector.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Extract attributes (content within [...])
        let mut remaining = part;
        while let Some(start) = remaining.find('[') {
            if let Some(end) = remaining[start..].find(']') {
                let attr_content = &remaining[start + 1..start + end];
                // Handle [attr=value] format
                let attr_name = attr_content.split('=').next().unwrap_or(attr_content);
                if !attr_name.is_empty() {
                    result.attributes.push(attr_name.to_string());
                }
                remaining = &remaining[start + end + 1..];
            } else {
                break;
            }
        }

        // Extract element name (everything before first [, ., or :)
        let element_end = part
            .find(|c| c == '[' || c == '.' || c == ':' || c == '#')
            .unwrap_or(part.len());
        let element_name = part[..element_end].trim();
        if !element_name.is_empty() && element_name.chars().next().is_some_and(|c| c.is_alphabetic()) {
            result.elements.push(element_name.to_string());
        }
    }

    result
}

/// Get valid selectors based on the expected types.
fn get_valid_selectors_for_types<'a>(parsed: &'a ParsedSelector, types: &[SelectorType]) -> Vec<&'a str> {
    let mut selectors = Vec::new();

    for selector_type in types {
        match selector_type {
            SelectorType::Element => {
                selectors.extend(parsed.elements.iter().map(std::string::String::as_str));
            }
            SelectorType::Attribute => {
                selectors.extend(parsed.attributes.iter().map(std::string::String::as_str));
            }
        }
    }

    selectors
}

/// Check if selector has the correct prefix considering the expected style.
fn check_prefix_with_style(selector: &str, prefixes: &[String], style: SelectorStyle) -> bool {
    if prefixes.is_empty() {
        return true;
    }

    prefixes.iter().any(|prefix| {
        if prefix.is_empty() {
            return true;
        }
        if let Some(rest) = selector.strip_prefix(prefix.as_str()) {
            // After prefix, we need either:
            // - End of selector (handled in has_selector_after_prefix)
            // - For camelCase: uppercase letter
            // - For kebab-case: hyphen
            if rest.is_empty() {
                return true;
            }
            let next_char = rest.chars().next().unwrap();
            match style {
                SelectorStyle::CamelCase => next_char.is_ascii_uppercase(),
                SelectorStyle::KebabCase => next_char == '-',
            }
        } else {
            false
        }
    })
}

/// Check if there's actual selector content after the prefix.
fn has_selector_after_prefix(selector: &str, prefixes: &[String], style: SelectorStyle) -> bool {
    if prefixes.is_empty() {
        return true;
    }

    prefixes.iter().any(|prefix| {
        if prefix.is_empty() {
            return true;
        }
        if let Some(rest) = selector.strip_prefix(prefix.as_str()) {
            if rest.is_empty() {
                return false; // Selector equals prefix exactly - not allowed
            }
            let next_char = rest.chars().next().unwrap();
            match style {
                SelectorStyle::CamelCase => next_char.is_ascii_uppercase(),
                SelectorStyle::KebabCase => next_char == '-' && rest.len() > 1,
            }
        } else {
            false
        }
    })
}

/// Check if selector matches the expected style.
/// Matches ESLint's SelectorValidator patterns:
/// - kebabCase: /^[a-z0-9]+(-[a-z0-9]+)*$/  (requires at least one hyphen for multi-word selectors)
/// - camelCase: /^[a-zA-Z0-9[\]]+$/
fn check_style(selector: &str, style: SelectorStyle) -> bool {
    match style {
        SelectorStyle::KebabCase => {
            // kebab-case regex: ^[a-z0-9]+(-[a-z0-9]+)*$
            // This requires at least one hyphen for the pattern to match
            if selector.is_empty() {
                return false;
            }
            let parts: Vec<&str> = selector.split('-').collect();
            if parts.len() < 2 {
                // No hyphen = not kebab-case
                return false;
            }
            parts.iter().all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        }
        SelectorStyle::CamelCase => {
            // camelCase regex: ^[a-zA-Z0-9[\]]+$
            // Note: brackets are included in the pattern but we're checking the extracted name
            !selector.is_empty() && selector.chars().all(|c| c.is_ascii_alphanumeric())
        }
    }
}

/// Format prefix list for error message: "\"app\", \"cd\" or \"ng\"".
fn format_prefix_list(prefixes: &[String]) -> String {
    if prefixes.is_empty() {
        return String::new();
    }
    if prefixes.len() == 1 {
        return format!("\"{}\"", prefixes[0]);
    }
    let last = prefixes.last().unwrap();
    let rest: Vec<_> = prefixes[..prefixes.len() - 1]
        .iter()
        .map(|p| format!("\"{}\"", p))
        .collect();
    format!("{} or \"{}\"", rest.join(", "), last)
}

/// Format prefix for "selectorAfterPrefix" message: "\"app\"".
fn format_prefix_for_message(prefixes: &[String]) -> String {
    if prefixes.is_empty() {
        return String::new();
    }
    format!("\"{}\"", prefixes[0])
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
        // Empty prefix array - prefix check skipped, only type/style validated
        (
            r"
            @Directive({
                selector: '[fooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "style": "camelCase", "prefix": [] }])),
        ),
        // Empty prefix string - prefix check skipped, only type/style validated
        (
            r"
            @Directive({
                selector: '[fooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "style": "camelCase", "prefix": "" }])),
        ),
        // Element selector with kebab-case
        (
            r"
            @Directive({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }])),
        ),
        // Complex selector with element and attributes
        (
            r"
            @Directive({
                selector: 'app-foo-bar[baz].app'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ng"], "style": "kebab-case" }])),
        ),
        // Attribute directive on element (button[app-foo-bar])
        (
            r"
            @Directive({
                selector: 'button[app-foo-bar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": ["attribute"], "prefix": ["app"], "style": "kebab-case" }])),
        ),
        // Template literal selector
        (
            r"
            @Directive({
                selector: `[app-foo-bar]`
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }])),
        ),
        // Multiple configs - element matches first
        (
            r"
            @Directive({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - attribute matches second
        (
            r"
            @Directive({
                selector: '[appFooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs with different prefixes
        (
            r"
            @Directive({
                selector: 'lib-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": ["app", "lib"], "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Type as array
        (
            r"
            @Directive({
                selector: '[app-foo-bar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": ["attribute"], "prefix": ["app"], "style": "kebab-case" }])),
        ),
        // Non-Angular Directive (should not trigger)
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
        // Component decorator (should not trigger for directive-selector)
        (
            r"
            @Component({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": ["element"], "prefix": ["bar"], "style": "kebab-case" }])),
        ),
    ];

    let fail = vec![
        // Wrong type (element instead of attribute)
        (
            r"
            @Directive({
                selector: 'app-highlight'
            })
            class HighlightDirective {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
        // Wrong prefix
        (
            r"
            @Directive({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": "bar", "style": "kebab-case" }])),
        ),
        // Wrong prefix with multiple allowed
        (
            r"
            @Directive({
                selector: '[app-foo-bar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "prefix": ["cd", "ng"], "style": "kebab-case" }])),
        ),
        // Wrong style (kebab-case instead of camelCase)
        (
            r"
            @Directive({
                selector: '[app-bar-foo]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }])),
        ),
        // Wrong style (camelCase instead of kebab-case)
        (
            r"
            @Directive({
                selector: 'appFooBar'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }])),
        ),
        // Selector equals prefix exactly (no content after prefix)
        (
            r"
            @Directive({
                selector: 'app'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }])),
        ),
        // Selector equals prefix exactly (camelCase style)
        (
            r"
            @Directive({
                selector: 'app'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "camelCase" }])),
        ),
        // Wrong type (attribute instead of element)
        (
            r"
            @Directive({
                selector: '[appFooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "camelCase" }])),
        ),
        // Multiple configs - neither matches style
        (
            r"
            @Directive({
                selector: 'appFooBar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - attribute style wrong
        (
            r"
            @Directive({
                selector: '[app-foo-bar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - wrong prefix for element
        (
            r"
            @Directive({
                selector: 'lib-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
    ];

    Tester::new(DirectiveSelector::NAME, DirectiveSelector::PLUGIN, pass, fail).test_and_snapshot();
}
