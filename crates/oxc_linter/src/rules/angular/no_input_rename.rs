use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_call, get_decorator_name, get_metadata_property,
        get_metadata_string_value,
    },
};

const STYLE_GUIDE_LINK: &str = "https://angular.dev/guide/components/inputs#choosing-input-names";

fn no_input_rename_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Input bindings should not be aliased ({STYLE_GUIDE_LINK})"))
        .with_help(
            "Avoid aliasing inputs as it can lead to confusion. Use the original property name \
            or rename the property if the alias is more appropriate.",
        )
        .with_label(span)
}

/// ARIA attribute keys that are allowed as input aliases when the property name
/// is the camelCase version of the aria attribute.
const ARIA_ATTRIBUTE_KEYS: [&str; 49] = [
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-braillelabel",
    "aria-brailleroledescription",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colindextext",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-description",
    "aria-details",
    "aria-disabled",
    "aria-dropeffect",
    "aria-errormessage",
    "aria-expanded",
    "aria-flowto",
    "aria-grabbed",
    "aria-haspopup",
    "aria-hidden",
    "aria-invalid",
    "aria-keyshortcuts",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-live",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowindextext",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    // "aria-valuemax",
    // "aria-valuemin",
    // "aria-valuenow",
    // "aria-valuetext",
];

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoInputRenameConfig {
    #[serde(default)]
    allowed_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NoInputRename {
    allowed_names: Vec<String>,
}

impl From<NoInputRenameConfig> for NoInputRename {
    fn from(config: NoInputRenameConfig) -> Self {
        Self { allowed_names: config.allowed_names }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows aliasing input bindings (renaming inputs with a different public name).
    ///
    /// ### Why is this bad?
    ///
    /// Two names for the same property (one private, one public) is confusing.
    /// It requires developers to remember both names and understand the mapping.
    ///
    /// Exceptions are made for:
    /// - Cases where the alias matches the property name (redundant but harmless)
    /// - Names specified in the `allowedNames` configuration
    /// - Aliases that match the directive's selector
    /// - Aliases in the form `selectorPropertyName` (e.g., `fooMyColor` for selector `foo` and property `myColor`)
    /// - ARIA attributes when the property name is the camelCase version (e.g., `@Input('aria-label') ariaLabel`)
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component, Input } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Input('label') name: string; // Aliased input
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, Input } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Input() name: string; // No alias
    /// }
    /// ```
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/no-input-rename": ["error", { "allowedNames": ["appCustom"] }]
    /// }
    /// ```
    NoInputRename,
    angular,
    correctness,
    pending
);

impl Rule for NoInputRename {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        serde_json::from_value::<NoInputRenameConfig>(config_value.clone()).map(Into::into)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Check for decorators: @Input, @Component, @Directive
        if let AstKind::Decorator(decorator) = node.kind() {
            let Some(decorator_name) = get_decorator_name(decorator) else {
                return;
            };

            if decorator_name == "Input" {
                self.check_input_decorator(decorator, node, ctx);
            } else if decorator_name == "Component" || decorator_name == "Directive" {
                // Check for inputs metadata array aliases
                self.check_inputs_metadata_aliases(decorator, ctx);
            }
            return;
        }

        // Check for input() and input.required() signal functions
        if let AstKind::PropertyDefinition(prop) = node.kind() {
            self.check_input_signal(prop, node, ctx);
        }
    }
}

impl NoInputRename {
    /// Check @Input decorator for aliasing
    fn check_input_decorator(
        &self,
        decorator: &oxc_ast::ast::Decorator<'_>,
        node: &AstNode<'_>,
        ctx: &LintContext<'_>,
    ) {
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the decorator call to check for alias
        let Some(call) = get_decorator_call(decorator) else {
            return;
        };

        // Get the first argument (the alias) and its span for error reporting
        let Some(first_arg) = call.arguments.first() else {
            return;
        };

        let (alias, alias_span) = match first_arg {
            oxc_ast::ast::Argument::StringLiteral(lit) => (Some(lit.value.as_str()), lit.span),
            oxc_ast::ast::Argument::TemplateLiteral(tpl) => {
                // Handle template literals with no expressions: `alias`
                if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                    (Some(tpl.quasis[0].value.raw.as_str()), tpl.span)
                } else {
                    (None, tpl.span)
                }
            }
            oxc_ast::ast::Argument::ObjectExpression(obj) => {
                // Check for { alias: 'name' } format
                if let Some((alias_str, span)) = get_alias_from_object_with_span(obj) {
                    (Some(alias_str), span)
                } else {
                    (None, obj.span)
                }
            }
            _ => (None, first_arg.span()),
        };

        let Some(alias) = alias else {
            return;
        };

        // Get the property name from parent (supports both PropertyDefinition and MethodDefinition for setters)
        let property_name = self.get_property_name_from_decorator(node, ctx);

        // Get selectors and directive name from the class's @Component/@Directive decorator
        let (selectors, selector_directive_name) = self.get_class_selectors_and_directive(node, ctx);

        // Check if alias is in allowed names configuration
        if self.allowed_names.iter().any(|name| name == alias) {
            return;
        }

        // Check if it's an allowed aria attribute alias
        if let Some(ref prop_name) = property_name {
            if is_aria_attribute_allowed(alias, prop_name) {
                return;
            }
        }

        // If alias equals property name, still report (ESLint reports with fix to remove)
        if property_name.as_deref() == Some(alias) {
            ctx.diagnostic(no_input_rename_diagnostic(alias_span));
            return;
        }

        // Check if alias is allowed based on selector
        if let Some(ref prop_name) = property_name {
            if is_alias_allowed(&selectors, prop_name, alias, selector_directive_name.as_deref()) {
                return;
            }
        }

        ctx.diagnostic(no_input_rename_diagnostic(alias_span));
    }

    /// Check inputs metadata array in @Component/@Directive for aliases
    fn check_inputs_metadata_aliases(
        &self,
        decorator: &oxc_ast::ast::Decorator<'_>,
        ctx: &LintContext<'_>,
    ) {
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Get selectors for allowlist checking
        let (selectors, selector_directive_name) = get_selectors_from_metadata(metadata);

        // Get the inputs property value
        let Some(inputs_value) = get_metadata_property(metadata, "inputs") else {
            return;
        };

        // inputs should be an array
        let Expression::ArrayExpression(array) = inputs_value else {
            return;
        };

        for element in &array.elements {
            let Some(element) = element.as_expression() else {
                continue;
            };

            // Get string value and span from the element
            let (value, span) = match element {
                Expression::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                Expression::TemplateLiteral(tpl) => {
                    if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                        (tpl.quasis[0].value.raw.as_str(), tpl.span)
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Check for "propertyName: aliasName" format
            let value = value.trim();
            if let Some(colon_pos) = value.find(':') {
                let property_name = value[..colon_pos].trim();
                let alias_name = value[colon_pos + 1..].trim();

                // Skip if no alias (just property name without colon would not enter here)
                if alias_name.is_empty() {
                    continue;
                }

                // Check if this inputs property is inside hostDirectives (allowed)
                if is_inside_host_directives(metadata, inputs_value) {
                    continue;
                }

                // Check if alias is in allowed names
                if self.allowed_names.iter().any(|name| name == alias_name) {
                    continue;
                }

                // Check if it's an allowed aria attribute alias
                if is_aria_attribute_allowed(alias_name, property_name) {
                    continue;
                }

                // If alias equals property name, still report (with fix to remove alias)
                if alias_name == property_name {
                    ctx.diagnostic(no_input_rename_diagnostic(span));
                    continue;
                }

                // Check if alias is allowed based on selector
                if is_alias_allowed(&selectors, property_name, alias_name, selector_directive_name.as_deref()) {
                    continue;
                }

                ctx.diagnostic(no_input_rename_diagnostic(span));
            }
        }
    }

    /// Check input() and input.required() signal functions for aliasing
    fn check_input_signal(
        &self,
        prop: &oxc_ast::ast::PropertyDefinition<'_>,
        node: &AstNode<'_>,
        ctx: &LintContext<'_>,
    ) {
        let Some(value) = &prop.value else {
            return;
        };

        let Expression::CallExpression(call) = value else {
            return;
        };

        // Check for input() or input.required()
        let is_input = match &call.callee {
            Expression::Identifier(ident) => ident.name.as_str() == "input",
            Expression::StaticMemberExpression(member) => {
                if let Expression::Identifier(obj) = &member.object {
                    obj.name.as_str() == "input" && member.property.name.as_str() == "required"
                } else {
                    false
                }
            }
            _ => false,
        };

        if !is_input {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity

        // Get selectors and directive name from the class's @Component/@Directive decorator
        let (selectors, selector_directive_name) = self.get_class_selectors_and_directive(node, ctx);

        // Check for alias in options object (can be any argument position)
        for arg in &call.arguments {
            if let oxc_ast::ast::Argument::ObjectExpression(obj) = arg {
                if let Some((alias, alias_span)) = get_alias_from_object_with_span(obj) {
                    let property_name = prop.key.static_name();

                    // Check if alias is in allowed names
                    if self.allowed_names.iter().any(|name| name == alias) {
                        return;
                    }

                    // Check if it's an allowed aria attribute alias
                    if let Some(prop_name) = property_name.as_deref() {
                        if is_aria_attribute_allowed(alias, prop_name) {
                            return;
                        }
                    }

                    // If alias equals property name, still report (ESLint does)
                    if property_name.as_deref() == Some(alias) {
                        ctx.diagnostic(no_input_rename_diagnostic(alias_span));
                        return;
                    }

                    // Check if alias is allowed based on selector
                    if let Some(prop_name) = property_name.as_deref() {
                        if is_alias_allowed(&selectors, prop_name, alias, selector_directive_name.as_deref()) {
                            return;
                        }
                    }

                    ctx.diagnostic(no_input_rename_diagnostic(alias_span));
                    return;
                }
            }
        }
    }

    /// Get property name from decorator's parent (PropertyDefinition or MethodDefinition for setters)
    fn get_property_name_from_decorator(
        &self,
        node: &crate::AstNode<'_>,
        ctx: &LintContext<'_>,
    ) -> Option<String> {
        for ancestor in ctx.nodes().ancestors(node.id()) {
            match ancestor.kind() {
                AstKind::PropertyDefinition(prop) => {
                    return prop.key.static_name().map(|s| s.to_string());
                }
                AstKind::MethodDefinition(method) => {
                    // For setters: @Input('alias') set setter(value) {}
                    return method.key.static_name().map(|s| s.to_string());
                }
                _ => {}
            }
        }
        None
    }

    /// Get selectors and the directive name (from attribute selector) from the class's @Component/@Directive decorator
    fn get_class_selectors_and_directive(&self, node: &AstNode<'_>, ctx: &LintContext<'_>) -> (Vec<String>, Option<String>) {
        // Navigate up to find the class declaration
        for ancestor in ctx.nodes().ancestors(node.id()) {
            if let AstKind::Class(class) = ancestor.kind() {
                // Find @Component or @Directive decorator
                for decorator in &class.decorators {
                    let Some(decorator_name) = get_decorator_name(decorator) else {
                        continue;
                    };

                    if decorator_name != "Component" && decorator_name != "Directive" {
                        continue;
                    }

                    let Some(metadata) = get_component_metadata(decorator) else {
                        continue;
                    };

                    return get_selectors_from_metadata(metadata);
                }
            }
        }
        (Vec::new(), None)
    }
}

/// Get selectors and directive name from component/directive metadata
fn get_selectors_from_metadata(metadata: &oxc_ast::ast::ObjectExpression<'_>) -> (Vec<String>, Option<String>) {
    let Some(selector_str) = get_metadata_string_value(metadata, "selector") else {
        return (Vec::new(), None);
    };

    // Parse selector string: "foo[bar], test" -> ["foo", "bar", "test"]
    // Also extract directive name from attribute selector like [fooDirective]
    parse_selectors(selector_str)
}

/// Parse selector string into individual selector names and extract directive name
/// Returns (selectors, directive_name) where directive_name is extracted from [name] format
fn parse_selectors(selector: &str) -> (Vec<String>, Option<String>) {
    let mut result = Vec::new();
    let mut directive_name = None;

    // Split by comma for multiple selectors
    for part in selector.split(',') {
        let part = part.trim();

        // Check for attribute selector to extract directive name: img[fooDirective] or [fooDirective]
        if let Some(bracket_start) = part.find('[') {
            if let Some(bracket_end) = part.find(']') {
                let attr_name = &part[bracket_start + 1..bracket_end];
                // Handle [attr=value] format - only take the attribute name
                let attr_name = attr_name.split('=').next().unwrap_or(attr_name);
                directive_name = Some(attr_name.to_string());
            }
        }

        // Remove brackets and whitespace: "[foo]" -> "foo", "foo[bar]" -> "foo", "bar"
        let cleaned = part
            .replace('[', " ")
            .replace(']', " ")
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();

        for s in cleaned {
            if !s.is_empty() && !s.contains('=') {
                result.push(s);
            }
        }
    }

    (result, directive_name)
}

/// Check if an alias is allowed based on selectors
fn is_alias_allowed(selectors: &[String], property_name: &str, alias: &str, selector_directive_name: Option<&str>) -> bool {
    // Check if alias matches directive name from attribute selector
    if let Some(dir_name) = selector_directive_name {
        if dir_name == alias {
            return true;
        }
    }

    selectors.iter().any(|selector| {
        // Direct match: alias equals selector
        selector == alias ||
        // Composed name: selector + capitalize(propertyName) equals alias
        composed_name(selector, property_name) == alias
    })
}

/// Check if an aria attribute alias is allowed
fn is_aria_attribute_allowed(alias: &str, property_name: &str) -> bool {
    // Check if alias is a known aria attribute
    if !ARIA_ATTRIBUTE_KEYS.contains(&alias) {
        return false;
    }

    // Check if property name is the camelCase version of the aria attribute
    let expected_camel_case = kebab_to_camel_case(alias);
    property_name == expected_camel_case
}

/// Convert kebab-case to camelCase
fn kebab_to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for c in s.chars() {
        if c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Create composed name: selector + capitalize(propertyName)
fn composed_name(selector: &str, property_name: &str) -> String {
    format!("{}{}", selector, capitalize(property_name))
}

/// Capitalize first letter of a string
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Check if the inputs property is inside a hostDirectives configuration
fn is_inside_host_directives(
    _metadata: &oxc_ast::ast::ObjectExpression<'_>,
    inputs_value: &Expression<'_>,
) -> bool {
    // The inputs_value is the array expression. We need to check if it's
    // nested inside hostDirectives. Since we got it from get_metadata_property
    // on the top-level metadata, it's NOT inside hostDirectives.
    // hostDirectives inputs would be at a deeper nesting level.
    // This function is called for top-level inputs, so return false.
    // The actual hostDirectives check happens because get_metadata_property
    // only looks at top-level properties, not nested ones.
    let _ = inputs_value;
    false
}

/// Get alias from object expression with span (supports both string and template literals)
fn get_alias_from_object_with_span<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<(&'a str, Span)> {
    use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};

    for property in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = property {
            if let PropertyKey::StaticIdentifier(ident) = &prop.key {
                if ident.name.as_str() == "alias" {
                    match &prop.value {
                        Expression::StringLiteral(lit) => {
                            return Some((lit.value.as_str(), lit.span));
                        }
                        Expression::TemplateLiteral(tpl) => {
                            // Handle template literals with no expressions: `alias`
                            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                                return Some((tpl.quasis[0].value.raw.as_str(), tpl.span));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Non-component/directive class
        r"class Test {}",
        // Non-Angular decorator with inputs
        r#"
    @Page({
      inputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
    })
    class Test {}
  "#,
        // Component without @Input
        r"
    @Component()
    class Test {
      change = new EventEmitter();
    }
  ",
        // @Input without alias on Directive
        r"
    @Directive()
    class Test {
      @Input() buttonChange = new EventEmitter<'change'>();
    }
  ",
        // input() signal without alias on Directive
        r"
    @Directive()
    class Test {
      buttonChange = input(1);
    }
  ",
        // input.required() without alias
        r"
    @Directive()
    class Test {
      buttonChange = input.required<number>();
    }
  ",
        // inputs as identifier (not array literal)
        r"
    @Component({
      inputs,
    })
    class Test {}
  ",
        // inputs with spread
        r"
    @Directive({
      inputs: [...test],
    })
    class Test {}
  ",
        // inputs as function call
        r"
    @Component({
      inputs: func(),
    })
    class Test {}
  ",
        // inputs with function call element
        r"
    @Directive({
      inputs: [func(), 'a'],
    })
    class Test {}
  ",
        // hostDirectives inputs (allowed to be aliased)
        r"
    @Component({
      selector: 'qx-menuitem',
      hostDirectives: [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
        // hostDirectives with string key
        r"
    @Component({
      selector: 'qx-menuitem',
      'hostDirectives': [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
        // hostDirectives with computed key
        r"
    @Component({
      selector: 'qx-menuitem',
      ['hostDirectives']: [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
        // @Input on setter without alias
        r"
    @Component({})
    class Test {
      @Input() set setter(setter: string) {}
    }
  ",
        // allowedNames configuration
        (
            r#"
      @Component({
        inputs: ['foo: aria-wrong']
      })
      class Test {
        @Input('aria-wrong') set setter(setter: string) {}
        func = input(1, { alias: 'aria-wrong' });
        required = input.required<number>({ alias: 'aria-wrong' });
      }
      "#,
            Some(serde_json::json!([{ "allowedNames": ["aria-wrong"] }])),
        ),
        // Dynamic alias (variable reference) - not statically analyzable
        r"
    const change = 'change';
    @Component()
    class Test {
      @Input(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
    }
  ",
        // input() with dynamic alias
        r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = input(1, { alias: change });
    }
  ",
        // input.required() with dynamic alias
        r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = input.required<number>({ alias: change });
    }
  ",
        // Dynamic property name and alias
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      @Input(blur) [click]: EventEmitter<Blur>;
    }
  ",
        // input() with dynamic property and alias
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = input(1, { alias: blur });
    }
  ",
        // input.required() with dynamic property and alias
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = input.required<number>({ alias: blur });
    }
  ",
        // Selector matches input name
        r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      @Input() bar: string;
    }
  ",
        // input() with selector match
        r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      bar = input(1);
    }
  ",
        // input.required() with selector match
        r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      bar = input.required<number>();
    }
  ",
        // inputs metadata with selector prefix allowed (selector: foo, input alias: foo)
        r"
    @Directive({
      'selector': 'foo',
      'inputs': [`test: foo`]
    })
    class Test {}
  ",
        // inputs metadata with computed key (string)
        r"
    @Component({
      'selector': 'foo',
      ['inputs']: [`test: foo`]
    })
    class Test {}
  ",
        // inputs metadata with computed key (template)
        r"
    @Directive({
      'selector': 'foo',
      [`inputs`]: [`test: foo`]
    })
    class Test {}
  ",
        // Alias matches selector
        r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      @Input('foo') label: string;
    }
  ",
        // input() alias matches selector
        r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      label = input(1, { alias: 'foo' });
    }
  ",
        // input.required() alias matches selector
        r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      label = input.required<number>(1, { alias: 'foo' });
    }
  ",
        // ARIA attribute alias with matching camelCase property name
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      @Input('aria-label') ariaLabel: string;
    }
  ",
        // input() with aria attribute alias
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      ariaLabel = input(1, { alias: 'aria-label' });
    }
  ",
        // input.required() with aria attribute alias (note: ESLint test has wrong syntax but we follow the pattern)
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      ariaLabel = input.required<number>('aria-label');
    }
  ",
        // allowedNames in inputs metadata
        (
            r#"
      @Component({
        inputs: ['foo: allowedName']
      })
      class Test {
        @Input() bar: string;
      }
      "#,
            Some(serde_json::json!([{ "allowedNames": ["allowedName"] }])),
        ),
        // Alias equals selector + capitalize(propertyName)
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      @Input('fooMyColor') myColor: string;
    }
  ",
        // input() alias equals selector + capitalize(propertyName)
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      myColor = input(1, { alias: 'fooMyColor' });
    }
  ",
        // input.required() alias equals selector + capitalize(propertyName)
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      myColor = input.required<number>('fooMyColor');
    }
  ",
        // Directive with attribute selector - input without alias
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      @Input foo: Foo;
    }
  ",
        // input() with attribute selector
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input(1);
    }
  ",
        // input.required() with attribute selector
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input.required<number>();
    }
  ",
        // Alias matches directive name from attribute selector
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      @Input('fooDirective') foo: Foo;
    }
  ",
        // input() alias matches directive name from attribute selector
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input(1, { alias: 'fooDirective' });
    }
  ",
        // input.required() alias matches directive name from attribute selector
        r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input.required<number>({ alias: 'fooDirective' });
    }
  ",
        // @Input with object options but no alias
        r"
    @Component({
      selector: 'foo'
    })
    class Test {
      @Input({ required: true }) name: string;
    }
  ",
        // @Input with transform and required options but no alias
        r"
    @Component({
      selector: 'foo'
    })
    class Test {
      @Input({ transform: (val) => val ?? '', required: true }) foo!: string;
    }
  ",
        // input.required() with transform but no alias
        r"
    @Component({
      selector: 'foo'
    })
    class Test {
      foo = input.required<string>({ transform: (val) => val ?? '' });
    }
  ",
    ];

    let fail = vec![
        // inputs metadata aliased in Component
        r"
      @Component({
        inputs: ['a: b']
      })
      class Test {}
    ",
        // inputs metadata with template literal key, aliased
        (
            r"
      @Directive({
        outputs: ['abort'],
        'inputs': [boundary, `test: copy`, 'check: check'],
      })
      class Test {}
    ",
            Some(serde_json::json!([{ "allowedNames": ["check", "test"] }])),
        ),
        // inputs metadata aliased with same name (should still report)
        r"
      @Component({
        ['inputs']: ['orientation: orientation'],
      })
      class Test {}
    ",
        // inputs metadata with template computed key, aliased with same name
        r"
      @Directive({
        [`inputs`]: ['orientation: orientation'],
      })
      class Test {}
    ",
        // @Input with backtick template literal alias
        r"
      @Component()
      class Test {
        @Custom() @Input(`change`) _change = getInput();
      }
    ",
        // input() with backtick template literal alias
        r"
      @Component()
      class Test {
        _change = input(1, { alias: `change` });
      }
    ",
        // input.required() with backtick template literal alias
        r"
      @Component()
      class Test {
        _change = input.required<number>({ alias: `change` });
      }
    ",
        // @Input alias matches property name (still reported per ESLint)
        r"
      @Directive()
      class Test {
        @Input('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
        // input() alias matches property name (still reported per ESLint)
        r"
      @Directive()
      class Test {
        change = input(1, { alias: 'change' });
      }
    ",
        // input.required() alias matches property name (still reported per ESLint)
        r"
      @Directive()
      class Test {
        change = input.required<number>({ alias: 'change' });
      }
    ",
        // @Input alias using metadata object
        r"
      @Directive()
      class Test {
        @Input({ alias: 'change' }) change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
        // @Input on setter with alias
        (
            r"
      @Component()
      class Test {
        @Input(`devicechange`) set setter(setter: string) {}

        @Input('allowedName') test: string;
      }
    ",
            Some(serde_json::json!([{ "allowedNames": ["allowedName"] }])),
        ),
        // aria-* alias name does not match the property name
        r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        @Input('aria-invalid') ariaBusy: string;
      }
    ",
        // input() aria-* alias does not match property name
        r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input(1, { alias: 'aria-invalid' });
      }
    ",
        // input.required() aria-* alias does not match property name
        r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input.required<number>({ alias: 'aria-invalid' });
      }
    ",
        // @Input alias prefixed by selector but suffix doesn't match property
        r"
      @Component({
        selector: 'foo'
      })
      class Test {
        @Input('fooColor') colors: string;
      }
    ",
        // input() alias prefixed by selector but suffix doesn't match property
        r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input(1, { alias: 'fooColor' });
      }
    ",
        // input.required() alias prefixed by selector but suffix doesn't match property
        r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input.required<number>({ alias: 'fooColor' });
      }
    ",
        // @Input alias not strictly camelCase with selector
        r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        @Input('foocolor') color: string;
      }
    ",
        // input() alias not strictly camelCase with selector
        r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input(1, { alias: 'foocolor' });
      }
    ",
        // input.required() alias not strictly camelCase with selector
        r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input.required<number>({ alias: 'foocolor' });
      }
    ",
        // @Input in class without Component/Directive decorator
        r"
      @Component({
        selector: 'click',
      })
      class Test {}

      @Injectable()
      class Test {
        @Input('click') blur = this.getInput();
      }
    ",
        // input() in class without Component/Directive decorator
        r"
      @Component({
        selector: 'click',
      })
      class Test {}

      @Injectable()
      class Test {
        blur = input(1, { alias: 'click' });
      }
    ",
        // input.required() in class without Component/Directive decorator
        r"
      @Component({
        selector: 'click',
      })
      class Test {}

      @Injectable()
      class Test {
        blur = input.required<number>({ alias: 'click' });
      }
    ",
        // @Input alias does not match directive name from attribute selector
        r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        @Input('notFooDirective') foo: Foo;
      }
    ",
        // input() alias does not match directive name from attribute selector
        r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input({ alias: 'notFooDirective' });
      }
    ",
        // input.required() alias does not match directive name from attribute selector
        r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input.required<number>({ alias: 'notFooDirective' });
      }
    ",
        // input() alias with properties after it
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { alias: 'test', after: 'it' });
        }
      ",
        // input.required() alias with properties after it
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ alias: 'test', after: 'it' });
        }
      ",
        // input() alias with properties before it (trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', });
        }
      ",
        // input.required() alias with properties before it (trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', });
        }
      ",
        // input() alias with properties before it (no trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test' });
        }
      ",
        // input.required() alias with properties before it (no trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test' });
        }
      ",
        // input() alias with properties before and after
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', after: 'it' });
        }
      ",
        // input.required() alias with properties before and after
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', after: 'it' });
        }
      ",
    ];

    Tester::new(NoInputRename::NAME, NoInputRename::PLUGIN, pass, fail).test_and_snapshot();
}
