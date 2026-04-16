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
        SelectorStyle, SelectorType,
        get_component_metadata, get_decorator_name, get_metadata_property,
    },
};

fn component_selector_type_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Component selector should be used as {expected}"))
        .with_help(format!(
            "Change the selector to use {expected} style (e.g., 'app-example' for element)"
        ))
        .with_label(span)
}

fn component_selector_prefix_diagnostic(span: Span, prefix_text: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Component selector should be prefixed with one of: {prefix_text}"
    ))
    .with_help("Add a prefix to the selector (e.g., 'app-example' with prefix 'app')")
    .with_label(span)
}

fn component_selector_style_diagnostic(span: Span, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Component selector should be {expected}"))
        .with_help(format!(
            "Use {expected} for the selector (e.g., 'app-example' for kebab-case)"
        ))
        .with_label(span)
}

fn component_selector_style_and_prefix_diagnostic(
    span: Span,
    style: &str,
    prefix_text: &str,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Component selector should be {style} and prefixed with one of: {prefix_text}"
    ))
    .with_help(format!(
        "Use {style} for the selector with a valid prefix (e.g., 'app-example')"
    ))
    .with_label(span)
}

fn component_selector_after_prefix_diagnostic(span: Span, prefix_text: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "There should be a selector after the {prefix_text} prefix"
    ))
    .with_label(span)
}

fn component_selector_shadow_dom_style_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "The selector of a ShadowDom-encapsulated component should be kebab-case",
    )
    .with_label(span)
}

// ── Config types ────────────────────────────────────────────────────────

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
    "kebab-case".to_string()
}

impl Default for SingleSelectorConfig {
    fn default() -> Self {
        Self {
            r#type: TypeConfig::Single("element".to_string()),
            prefix: PrefixConfig::None,
            style: default_style(),
        }
    }
}

/// Full configuration - can be a single config object or array of config objects.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ComponentSelectorConfig {
    Single(SingleSelectorConfig),
    Multiple(Vec<SingleSelectorConfig>),
}

impl Default for ComponentSelectorConfig {
    fn default() -> Self {
        ComponentSelectorConfig::Single(SingleSelectorConfig::default())
    }
}

// ── Parsed rule types ───────────────────────────────────────────────────

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
            "camelCase" => SelectorStyle::CamelCase,
            _ => SelectorStyle::KebabCase,
        };
        Self {
            selector_types,
            prefixes: config.prefix.as_vec(),
            style,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComponentSelector {
    rules: Vec<ParsedSelectorRule>,
}

impl Default for ComponentSelector {
    fn default() -> Self {
        Self {
            rules: vec![ParsedSelectorRule {
                selector_types: vec![SelectorType::Element],
                prefixes: vec![],
                style: SelectorStyle::KebabCase,
            }],
        }
    }
}

impl From<ComponentSelectorConfig> for ComponentSelector {
    fn from(config: ComponentSelectorConfig) -> Self {
        let rules = match config {
            ComponentSelectorConfig::Single(single) => vec![ParsedSelectorRule::from(single)],
            ComponentSelectorConfig::Multiple(multiple) => {
                multiple.into_iter().map(ParsedSelectorRule::from).collect()
            }
        };
        Self { rules }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Validates Angular component selectors against configured type, prefix, and style rules.
    ///
    /// ### Why is this bad?
    ///
    /// Consistent selector conventions help:
    /// - Avoid naming collisions with native HTML elements or third-party components
    /// - Easily identify your application's components
    /// - Maintain consistency across the codebase
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/component-selector": ["error", {
    ///     "type": "element",
    ///     "prefix": "app",
    ///     "style": "kebab-case"
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
    /// With configuration `{ "type": "element", "prefix": "app", "style": "kebab-case" }`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// @Component({ selector: 'example' })  // Missing prefix
    /// @Component({ selector: 'AppExample' })  // Wrong style
    /// @Component({ selector: '[appExample]' })  // Wrong type
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// @Component({ selector: 'app-example' })
    /// ```
    ComponentSelector,
    angular,
    pedantic,
    pending,
    config = ComponentSelectorConfig
);

impl Rule for ComponentSelector {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        serde_json::from_value::<ComponentSelectorConfig>(config_value.clone()).map(Into::into)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Component decorator
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Component" {
            return;
        }

        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Get the selector property value expression (for accurate span reporting)
        let Some(selector_expr) = get_metadata_property(metadata, "selector") else {
            return;
        };

        // Extract the string value from the selector expression.
        // If the selector is a variable reference or shorthand property, skip validation
        // (matches ESLint behavior where parseSelectorNode returns null for non-literals).
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

        // Get the span for error reporting
        let selector_span = selector_expr.span();

        // Parse the CSS selector to extract element and attribute names
        let parsed = parse_css_selector(selector_raw);
        if parsed.elements.is_empty() && parsed.attributes.is_empty() {
            return;
        }

        // Check if ShadowDom encapsulation is used
        let is_shadow_dom = has_shadow_dom_encapsulation(metadata);

        // Determine the actual selector type (for multi-config dispatch)
        let actual_type = get_actual_selector_type(&parsed);

        // For multi-config (rules.len() > 1), pick the applicable rule based on actual type
        let applicable_rules: Vec<&ParsedSelectorRule> = if self.rules.len() > 1 {
            if let Some(actual) = actual_type {
                self.rules
                    .iter()
                    .filter(|r| r.selector_types.contains(&actual))
                    .collect()
            } else {
                return;
            }
        } else {
            self.rules.iter().collect()
        };

        if applicable_rules.is_empty() {
            return;
        }

        // For each applicable rule, compute the check results (matching ESLint's checkSelector)
        // Then apply the error priority logic per rule.
        for rule in &applicable_rules {
            if rule.selector_types.is_empty() {
                continue;
            }

            let valid_selectors = get_valid_selectors_for_types(&parsed, &rule.selector_types);

            // Determine effective style (ShadowDom forces kebab-case)
            let style_overridden = is_shadow_dom && rule.style != SelectorStyle::KebabCase;
            let effective_style = if style_overridden {
                SelectorStyle::KebabCase
            } else {
                rule.style
            };

            // Compute all checks at once (matching ESLint's checkSelector return)
            let has_expected_type = !valid_selectors.is_empty();

            let has_expected_prefix = rule.prefixes.is_empty()
                || valid_selectors
                    .iter()
                    .any(|sel| check_prefix_with_style(sel, &rule.prefixes, effective_style));

            let has_expected_style = valid_selectors
                .iter()
                .any(|sel| check_style(sel, effective_style));

            let has_selector_after = rule.prefixes.is_empty()
                || valid_selectors
                    .iter()
                    .any(|sel| has_selector_after_prefix(sel, &rule.prefixes));

            // If all checks pass, we're good (no error for this rule)
            let all_pass = has_expected_type
                && has_expected_prefix
                && has_expected_style
                && has_selector_after;

            if all_pass {
                // ShadowDom-specific: selector must contain a hyphen
                // This check is done AFTER checkSelector but BEFORE error reporting,
                // matching ESLint's flow.
                if style_overridden {
                    let has_hyphen = parsed.elements.iter().any(|elem| elem.contains('-'));
                    if !has_hyphen {
                        ctx.diagnostic(component_selector_shadow_dom_style_diagnostic(
                            selector_span,
                        ));
                        return;
                    }
                }
                // All checks passed
                return;
            }

            // ShadowDom hyphen check (before error priority logic, matching ESLint)
            if style_overridden {
                let has_hyphen = parsed.elements.iter().any(|elem| elem.contains('-'));
                if !has_hyphen {
                    ctx.diagnostic(component_selector_shadow_dom_style_diagnostic(
                        selector_span,
                    ));
                    return;
                }
            }

            // Error priority logic (matching ESLint's component-selector.ts flow)
            // Priority 1: type
            if !has_expected_type {
                let type_str = if rule.selector_types.contains(&SelectorType::Element) {
                    "an element"
                } else {
                    "an attribute"
                };
                ctx.diagnostic(component_selector_type_diagnostic(selector_span, type_str));
                return;
            }

            // Priority 2: selector after prefix
            if !has_selector_after && !rule.prefixes.is_empty() {
                let prefix_text = format_prefix_for_message(&rule.prefixes);
                ctx.diagnostic(component_selector_after_prefix_diagnostic(
                    selector_span,
                    &prefix_text,
                ));
                return;
            }

            // Priority 3: style
            if !has_expected_style {
                if style_overridden {
                    ctx.diagnostic(component_selector_shadow_dom_style_diagnostic(
                        selector_span,
                    ));
                } else if !has_expected_prefix && !rule.prefixes.is_empty() {
                    // Both style and prefix wrong → combined error
                    let style_str = match effective_style {
                        SelectorStyle::KebabCase => "kebab-case",
                        SelectorStyle::CamelCase => "camelCase",
                    };
                    let prefix_text = format_prefix_list(&rule.prefixes);
                    ctx.diagnostic(component_selector_style_and_prefix_diagnostic(
                        selector_span,
                        style_str,
                        &prefix_text,
                    ));
                } else {
                    let style_str = match effective_style {
                        SelectorStyle::KebabCase => "kebab-case",
                        SelectorStyle::CamelCase => "camelCase",
                    };
                    ctx.diagnostic(component_selector_style_diagnostic(
                        selector_span, style_str,
                    ));
                }
                return;
            }

            // Priority 4: prefix
            if !has_expected_prefix && !rule.prefixes.is_empty() {
                let prefix_text = format_prefix_list(&rule.prefixes);
                ctx.diagnostic(component_selector_prefix_diagnostic(
                    selector_span,
                    &prefix_text,
                ));
                return;
            }
        }
    }
}

// ── CSS selector parsing ────────────────────────────────────────────────

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
        if !element_name.is_empty()
            && element_name
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic())
        {
            result.elements.push(element_name.to_string());
        }
    }

    result
}

/// Get valid selectors based on the expected types.
fn get_valid_selectors_for_types<'a>(
    parsed: &'a ParsedSelector,
    types: &[SelectorType],
) -> Vec<&'a str> {
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

/// Determine the actual selector type from the parsed selector.
/// Used for multi-config dispatch to find which config applies.
/// Matches ESLint's getActualSelectorType: attributes take priority over elements.
fn get_actual_selector_type(parsed: &ParsedSelector) -> Option<SelectorType> {
    // Attribute selectors take priority (matching ESLint behavior)
    if !parsed.attributes.is_empty() {
        return Some(SelectorType::Attribute);
    }

    if !parsed.elements.is_empty() {
        return Some(SelectorType::Element);
    }

    None
}

/// Check if the metadata has `encapsulation: ViewEncapsulation.ShadowDom`.
fn has_shadow_dom_encapsulation(metadata: &oxc_ast::ast::ObjectExpression<'_>) -> bool {
    let Some(encapsulation_expr) = get_metadata_property(metadata, "encapsulation") else {
        return false;
    };

    // Match: ViewEncapsulation.ShadowDom
    if let oxc_ast::ast::Expression::StaticMemberExpression(member) = encapsulation_expr {
        if let oxc_ast::ast::Expression::Identifier(obj) = &member.object {
            return obj.name.as_str() == "ViewEncapsulation"
                && member.property.name.as_str() == "ShadowDom";
        }
    }

    false
}

/// Check if selector has the correct prefix considering the expected style.
/// Matches ESLint's SelectorValidator.prefix behavior:
/// - For camelCase: after prefix, the next char must equal its own toUpperCase()
///   (this is true for uppercase letters AND non-alphabetic chars like '-', digits)
/// - For kebab-case: after prefix, the next char must be '-'
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
            // - End of selector (exact match - handled later by selectorAfterPrefix)
            // - For camelCase: char equals its own uppercase (letters A-Z, or non-alpha)
            // - For kebab-case: hyphen
            if rest.is_empty() {
                return true;
            }
            let next_char = rest.chars().next().unwrap();
            match style {
                SelectorStyle::CamelCase => {
                    // ESLint: selectorAfterPrefix[0] === selectorAfterPrefix[0].toUpperCase()
                    // This is true for uppercase letters, digits, hyphens, etc.
                    // Only false for lowercase letters a-z.
                    !next_char.is_ascii_lowercase()
                }
                SelectorStyle::KebabCase => next_char == '-',
            }
        } else {
            false
        }
    })
}

/// Check if there's actual selector content after the prefix.
/// Matches ESLint's SelectorValidator.selectorAfterPrefix behavior:
/// - If the prefix matches, there must be content after it
/// - If the prefix doesn't match at all, this check passes (returns true)
///   because the prefix mismatch is caught by the prefix check instead
fn has_selector_after_prefix(selector: &str, prefixes: &[String]) -> bool {
    if prefixes.is_empty() {
        return true;
    }

    prefixes.iter().any(|prefix| {
        if prefix.is_empty() {
            return true;
        }
        if let Some(rest) = selector.strip_prefix(prefix.as_str()) {
            // Prefix matched - there must be content after it
            !rest.is_empty()
        } else {
            // Prefix didn't match at all - this check passes
            // (the prefix check will catch the mismatch separately)
            true
        }
    })
}

/// Check if selector matches the expected style.
/// Matches ESLint's SelectorValidator patterns:
/// - kebabCase: /^[a-z0-9]+(-[a-z0-9]+)*$/
/// - camelCase: /^[a-zA-Z0-9[\]]+$/
fn check_style(selector: &str, style: SelectorStyle) -> bool {
    match style {
        SelectorStyle::KebabCase => {
            // kebab-case regex: ^[a-z0-9]+(-[a-z0-9]+)*$
            if selector.is_empty() {
                return false;
            }
            // Split on hyphens and validate each part
            let parts: Vec<&str> = selector.split('-').collect();
            parts.iter().all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        }
        SelectorStyle::CamelCase => {
            // camelCase regex: ^[a-zA-Z0-9[\]]+$
            // Note: brackets are included in the pattern but we're checking extracted names
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
        // Valid kebab-case element with prefix
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-example',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Multiple allowed prefixes
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'my-example',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "my"], "style": "kebab-case" }]),
            ),
        ),
        // No prefix requirement
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'example',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(serde_json::json!([{ "type": "element", "style": "kebab-case" }])),
        ),
        // Note: ESLint doesn't verify import source either, so @Component from
        // any import is checked. The rule only skips non-Component decorators.
        // Variable selector name (should skip - not a static string)
        (
            r"
            const selectorName = 'appFooBar';
            @Component({
                selector: selectorName
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Shorthand selector property (should skip)
        (
            r"
            const selecto = 'appFooBar';
            @Component({
                selector,
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Template literal selector
        (
            r"
            @Component({
                selector: `[appFooBar]`
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        // Multiline template literal selector
        (
            r"
            @Component({
                selector: `
                  [appFooBar]
                `
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        // ShadowDom encapsulation with kebab-case (valid)
        (
            r"
            @Component({
                selector: `app-foo-bar`,
                encapsulation: ViewEncapsulation.ShadowDom
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        // Directive decorator (should not trigger for component-selector)
        (
            r"
            @Directive({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["bar"], "style": "kebab-case" }]),
            ),
        ),
        // Complex selector
        (
            r"
            @Component({
                selector: 'app-foo-bar[baz].app'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        // Single config array - element
        (
            r"
            @Component({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[{ "type": "element", "prefix": "app", "style": "kebab-case" }]])),
        ),
        // Single config array - attribute
        (
            r"
            @Component({
                selector: '[appFooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[{ "type": "attribute", "prefix": "app", "style": "camelCase" }]])),
        ),
        // Multiple configs - element matches
        (
            r"
            @Component({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - attribute matches
        (
            r"
            @Component({
                selector: '[appFooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
    ];

    let fail = vec![
        // Wrong type (attribute instead of element)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: '[appExample]',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Missing prefix
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'example',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Wrong style (PascalCase instead of kebab-case)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'AppExample',
                template: ''
            })
            class ExampleComponent {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Missing prefix (foo-bar when sg required)
        (
            r"
            @Component({
                selector: 'foo-bar'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }]),
            ),
        ),
        // Wrong prefix (app- when sg required)
        (
            r"
            @Component({
                selector: 'app-foo-bar'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }]),
            ),
        ),
        // Attribute with wrong prefix
        (
            r"
            @Component({
                selector: '[app-foo-bar]'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        // Complex selector wrong prefix
        (
            r"
            @Component({
                selector: 'app-foo-bar[baz].app'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["foo", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        // Wrong style (kebab-case instead of camelCase for attribute)
        (
            r"
            @Component({
                selector: '[ng-bar-foo]'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "ng", "style": "camelCase" }]),
            ),
        ),
        // Style and prefix failure (camelCase element when kebab-case required)
        (
            r"
            @Component({
                selector: 'appFooBar'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Selector equals prefix exactly (kebab-case)
        (
            r"
            @Component({
                selector: 'app'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        // Selector equals prefix exactly (camelCase)
        (
            r"
            @Component({
                selector: 'app'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "style": "camelCase", "prefix": "app" }]),
            ),
        ),
        // Wrong type (attribute instead of element with camelCase)
        (
            r"
            @Component({
                selector: '[appFooBar]'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        // ShadowDom with wrong style (not kebab-case)
        (
            r"
            @Component({
                encapsulation: ViewEncapsulation.ShadowDom,
                selector: 'appFooBar'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        // ShadowDom without hyphen
        (
            r"
            @Component({
                encapsulation: ViewEncapsulation.ShadowDom,
                selector: 'appselector'
            })
            class Test {}
            ",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        // Multiple configs - element wrong style
        (
            r"
            @Component({
                selector: 'appFooBar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - attribute wrong style
        (
            r"
            @Component({
                selector: '[app-foo-bar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - element wrong prefix
        (
            r"
            @Component({
                selector: 'lib-foo-bar'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
        // Multiple configs - attribute wrong prefix
        (
            r"
            @Component({
                selector: '[libFooBar]'
            })
            class Test {}
            ",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" }
            ]])),
        ),
    ];

    Tester::new(ComponentSelector::NAME, ComponentSelector::PLUGIN, pass, fail).test_and_snapshot();
}
