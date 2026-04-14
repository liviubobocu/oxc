use oxc_ast::AstKind;
use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{get_component_metadata, get_decorator_name},
};

fn no_input_prefix_diagnostic(span: Span, prefix: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Input should not be prefixed with `{prefix}`"))
        .with_help(format!(
            "Rename the input to not start with `{prefix}`. Inputs should use \
            descriptive names without unnecessary prefixes that can make the code \
            harder to read or inconsistent with other inputs."
        ))
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoInputPrefixConfig {
    /// Prefixes that should not be used for input names
    #[serde(default = "default_prefixes")]
    prefixes: Vec<String>
}

fn default_prefixes() -> Vec<String> {
    vec!["on".to_string()]
}

impl Default for NoInputPrefixConfig {
    fn default() -> Self {
        Self { prefixes: default_prefixes() }
    }
}

#[derive(Debug, Clone)]
pub struct NoInputPrefix {
    prefixes: Vec<String>
}

impl Default for NoInputPrefix {
    fn default() -> Self {
        Self { prefixes: default_prefixes() }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows inputs from having certain prefixes.
    ///
    /// ### Why is this bad?
    ///
    /// Certain prefixes like "on" are often associated with events (like onClick),
    /// not inputs. Using "on" prefix for inputs can be confusing because:
    /// - It suggests the property is an event handler, not a data input
    /// - It creates inconsistency in the component's API
    /// - It goes against Angular naming conventions
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/no-input-prefix": ["error", {
    ///     "prefixes": ["on", "is", "can"]
    ///   }]
    /// }
    /// ```
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with default config):
    /// ```typescript
    /// import { Component, Input } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Input() onSelect: (item: any) => void; // Prefixed with 'on'
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, Input, Output, EventEmitter } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Input() selectedItem: any;
    ///   @Output() select = new EventEmitter(); // Events use @Output
    /// }
    /// ```
    NoInputPrefix,
    angular,
    style,
    pending,
    config = NoInputPrefixConfig
);

impl Rule for NoInputPrefix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        let config: NoInputPrefixConfig = serde_json::from_value(config_value.clone())?;
        Ok(Self { prefixes: config.prefixes })
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        match decorator_name {
            // Pattern 1: @Input() decorator on properties
            "Input" => {
                self.check_input_decorator(node, decorator, ctx);
            }
            // Pattern 2: @Component or @Directive with inputs metadata array
            "Component" | "Directive" => {
                self.check_inputs_metadata(decorator, ctx);
            }
            _ => {}
        }
    }
}

impl NoInputPrefix {
    /// Check @Input() decorator on properties for disallowed prefixes
    fn check_input_decorator<'a>(
        &self,
        node: &AstNode<'a>,
        decorator: &oxc_ast::ast::Decorator<'a>,
        ctx: &LintContext<'a>,
    ) {
        // ESLint matches @Input on any class, not just Component/Directive
        // So we skip the class check to match ESLint behavior

        // Get the property name and its span
        let Some((input_name, property_span)) = get_decorated_property_name_with_span(node, ctx)
        else {
            return;
        };

        // Check for alias in decorator arguments (returns (alias_name, alias_span) if present)
        let alias_info = get_input_alias_with_span(decorator);

        // Check property name first
        for prefix in &self.prefixes {
            if starts_with_prefix(&input_name, prefix) {
                // Report on property key span (matching ESLint)
                ctx.diagnostic(no_input_prefix_diagnostic(property_span, prefix));
                return;
            }
        }

        // Check alias if present
        if let Some((alias, alias_span)) = alias_info {
            for prefix in &self.prefixes {
                if starts_with_prefix(&alias, prefix) {
                    // Report on alias value span (matching ESLint)
                    ctx.diagnostic(no_input_prefix_diagnostic(alias_span, prefix));
                    return;
                }
            }
        }
    }

    /// Check inputs metadata array in @Component/@Directive for disallowed prefixes
    fn check_inputs_metadata<'a>(
        &self,
        decorator: &oxc_ast::ast::Decorator<'a>,
        ctx: &LintContext<'a>,
    ) {
        // Get the metadata object from the decorator
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Find the inputs property (supports identifier, string literal, and computed keys)
        let inputs_array = find_inputs_array(metadata);
        let Some(inputs_array) = inputs_array else {
            return;
        };

        // Check each element in the inputs array
        for element in &inputs_array.elements {
            // Only check string literals and template literals
            let (input_string, span) = match element {
                ArrayExpressionElement::StringLiteral(lit) => {
                    (lit.value.as_str(), lit.span)
                }
                ArrayExpressionElement::TemplateLiteral(tpl)
                    if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
                {
                    (tpl.quasis[0].value.raw.as_str(), tpl.span)
                }
                _ => continue, // Skip non-literal elements (identifiers, spread, function calls)
            };

            // Parse the input string: 'propertyName' or 'propertyName: aliasName'
            let trimmed = input_string.replace(char::is_whitespace, "");
            let parts: Vec<&str> = trimmed.split(':').collect();

            let property_name = parts.first().copied().unwrap_or("");
            let alias_name = parts.get(1).copied().unwrap_or("");

            // Check both property name and alias for disallowed prefixes
            for prefix in &self.prefixes {
                if starts_with_prefix(property_name, prefix)
                    || (!alias_name.is_empty() && starts_with_prefix(alias_name, prefix))
                {
                    ctx.diagnostic(no_input_prefix_diagnostic(span, prefix));
                    break; // Only report once per element
                }
            }
        }
    }
}

/// Find the inputs array in the metadata object.
/// Supports: `inputs: [...]`, `'inputs': [...]`, `['inputs']: [...]`, `[\`inputs\`]: [...]`
fn find_inputs_array<'a>(
    metadata: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<&'a oxc_ast::ast::ArrayExpression<'a>> {
    for prop in &metadata.properties {
        let ObjectPropertyKind::ObjectProperty(obj_prop) = prop else {
            continue;
        };

        let is_inputs_key = match &obj_prop.key {
            // Static identifier: inputs
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str() == "inputs",
            // String literal (non-computed): 'inputs'
            PropertyKey::StringLiteral(lit) => lit.value.as_str() == "inputs",
            // Computed template literal: [`inputs`]
            PropertyKey::TemplateLiteral(template) => {
                // Only match if it's a static template (no expressions)
                if template.expressions.is_empty() && template.quasis.len() == 1 {
                    template.quasis.first().is_some_and(|q| q.value.raw.as_str() == "inputs")
                } else {
                    false
                }
            }
            _ => {
                // Check for computed string literal: ['inputs']
                if obj_prop.computed {
                    match obj_prop.key.as_expression() {
                        Some(Expression::StringLiteral(lit)) => lit.value.as_str() == "inputs",
                        Some(Expression::TemplateLiteral(tpl))
                            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
                        {
                            tpl.quasis.first().is_some_and(|q| q.value.raw.as_str() == "inputs")
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            }
        };

        if is_inputs_key {
            // Return the array expression if the value is an array
            if let Expression::ArrayExpression(arr) = &obj_prop.value {
                return Some(arr.as_ref());
            }
        }
    }
    None
}

fn get_decorated_property_name_with_span<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Option<(String, Span)> {
    // The parent of the decorator should be the property definition
    let parent = ctx.nodes().parent_node(node.id());

    match parent.kind() {
        AstKind::PropertyDefinition(prop) => get_property_key_name_with_span(&prop.key),
        AstKind::AccessorProperty(prop) => get_property_key_name_with_span(&prop.key),
        AstKind::MethodDefinition(method) => get_property_key_name_with_span(&method.key),
        _ => None,
    }
}

/// Extract name and span from a property key.
/// Supports identifiers, string literals, and template literals.
fn get_property_key_name_with_span(key: &PropertyKey<'_>) -> Option<(String, Span)> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some((ident.name.to_string(), ident.span)),
        PropertyKey::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        PropertyKey::TemplateLiteral(tpl) if tpl.expressions.is_empty() && tpl.quasis.len() == 1 => {
            Some((tpl.quasis[0].value.raw.to_string(), tpl.span))
        }
        _ => None,
    }
}

fn get_input_alias_with_span(decorator: &oxc_ast::ast::Decorator<'_>) -> Option<(String, Span)> {
    let call_expr = match &decorator.expression {
        oxc_ast::ast::Expression::CallExpression(call) => call,
        _ => return None,
    };

    // @Input('alias'), @Input(`alias`), or @Input({ alias: 'alias' })
    let first_arg = call_expr.arguments.first()?;

    match first_arg {
        oxc_ast::ast::Argument::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        oxc_ast::ast::Argument::TemplateLiteral(tpl)
            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
        {
            Some((tpl.quasis[0].value.raw.to_string(), tpl.span))
        }
        oxc_ast::ast::Argument::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop
                    && let oxc_ast::ast::PropertyKey::StaticIdentifier(key) = &obj_prop.key
                    && key.name == "alias"
                {
                    // Handle both string literal and template literal alias values
                    match &obj_prop.value {
                        Expression::StringLiteral(lit) => {
                            return Some((lit.value.to_string(), lit.span));
                        }
                        Expression::TemplateLiteral(tpl)
                            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
                        {
                            return Some((tpl.quasis[0].value.raw.to_string(), tpl.span));
                        }
                        _ => {}
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn starts_with_prefix(name: &str, prefix: &str) -> bool {
    if !name.starts_with(prefix) {
        return false;
    }

    // If the name equals the prefix exactly (end of string case), it matches
    if name.len() == prefix.len() {
        return true;
    }

    // Check that the character after the prefix is NOT lowercase
    // This matches: onX (uppercase), on1 (digit), on- (non-alphanumeric)
    name.chars().nth(prefix.len()).is_some_and(|c| !c.is_ascii_lowercase())
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Input without forbidden prefix
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input() value: string;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Input with "on" in the middle (not prefix)
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input() selectedOption: string;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Custom prefix config - input without forbidden prefix
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input() onSelect: any;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["can", "is"] }])),
        ),
        // Plain class (no Angular decorator)
        (
            r"class Test {}",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Non-Angular decorator (@Page instead of @Component)
        (
            r"
            @Page({
                inputs: ['on', onChange, `onLine`, 'on: on2', 'offline: on', ...onCheck, onInput()],
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Component without @Input decorator - property without prefix
        (
            r"
            @Component()
            class Test {
                on = new EventEmitter();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input property without forbidden prefix
        (
            r"
            @Directive()
            class Test {
                @Input() buttonChange = new EventEmitter<'on'>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Uppercase prefix (case-sensitive)
        (
            r"
            @Component()
            class Test {
                @Input() On = new EventEmitter<{ on: onType }>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Template literal alias without forbidden prefix
        (
            r"
            @Directive()
            class Test {
                @Input(`one`) ontype = new EventEmitter<{ bar: string, on: boolean }>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Object alias without forbidden prefix
        (
            r"
            @Directive()
            class Test {
                @Input({ alias: `one` }) ontype = new EventEmitter<{ bar: string, on: boolean }>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // String alias without forbidden prefix
        (
            r"
            @Component()
            class Test {
                @Input('oneProp') common = new EventEmitter<ComplextOn>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Object alias with string value without forbidden prefix
        (
            r"
            @Component()
            class Test {
                @Input({ alias: 'oneProp' }) common = new EventEmitter<ComplextOn>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // ALL CAPS property name (case-sensitive)
        (
            r"
            @Directive()
            class Test<On> {
                @Input() ON = new EventEmitter<On>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Variable reference as alias (not a string literal)
        (
            r"
            const on = 'on';
            @Component()
            class Test {
                @Input(on) touchMove: EventEmitter<{ action: 'on' | 'off' }> = new EventEmitter<{ action: 'on' | 'off' }>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Computed property key (not a string literal)
        (
            r"
            const test = 'on';
            const on = 'on';
            @Directive()
            class Test {
                @Input(test) [on]: EventEmitter<OnTest>;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Property not starting with prefix (initializer contains prefix)
        (
            r"
            @Component()
            class Test {
                @Input() notOn: string = 'on';
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // inputs metadata with valid names (string key)
        (
            r"
            @Component({
                selector: 'foo',
                'inputs': [`test: foo`]
            })
            class Test {}
            ",
            None,
        ),
        // inputs metadata with computed string key
        (
            r"
            @Directive({
                selector: 'foo',
                ['inputs']: [`test: foo`]
            })
            class Test {}
            ",
            None,
        ),
        // inputs metadata with computed template key
        (
            r"
            @Component({
                selector: 'foo',
                [`inputs`]: [`test: foo`]
            })
            class Test {}
            ",
            None,
        ),
        // Setter with valid name
        (
            r"
            @Directive({
                selector: 'foo',
            })
            class Test {
                @Input() set 'setter'(_v: any) {}
            }
            ",
            None,
        ),
    ];

    let fail = vec![
        // Input with 'on' prefix (via @Input decorator)
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input() onSelect: any;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Input with 'on' prefix via string alias
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input('onSelect') select: any;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // Custom prefix config
        (
            r"
            import { Component, Input } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {
                @Input() canEdit: boolean;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["can", "is"] }])),
        ),
        // Directive with forbidden prefix
        (
            r"
            import { Directive, Input } from '@angular/core';
            @Directive({
                selector: '[appTest]'
            })
            class TestDirective {
                @Input() onHover: any;
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // inputs metadata property named "on" in @Component
        (
            r"
            @Component({
                inputs: ['on']
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // inputs metadata with aliased value "on" in @Directive
        (
            r"
            @Directive({
                outputs: [onCredit],
                'inputs': [onLevel, `test: on`, onFunction()],
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // inputs metadata with computed string key and "onTest" property
        (
            r"
            @Component({
                ['inputs']: ['onTest: test', ...onArray],
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // inputs metadata with computed template key and "onTest" property
        (
            r"
            @Directive({
                [`inputs`]: ['onTest: test', ...onArray],
            })
            class Test {}
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input property named "on" in @Component
        (
            r"
            @Component()
            class Test {
                @Input() on: EventEmitter<any> = new EventEmitter<{}>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input property with string literal key 'onPrefix'
        (
            r"
            @Directive()
            class Test {
                @Input() @Custom('on') 'onPrefix' = new EventEmitter<void>();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input with template literal alias `on`
        (
            r"
            @Component()
            class Test {
                @Custom() @Input(`on`) _on = getInput();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input with object metadata alias `on`
        (
            r"
            @Component()
            class Test {
                @Custom() @Input({ required: true, alias: `on` }) _on = getInput();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input with string alias 'onPrefix'
        (
            r"
            @Directive()
            class Test {
                @Input('onPrefix') _on = (this.subject$ as Subject<{on: boolean}>).pipe();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input with object metadata alias 'onPrefix'
        (
            r"
            @Directive()
            class Test {
                @Input({ alias: 'onPrefix', required: true }) _on = (this.subject$ as Subject<{on: boolean}>).pipe();
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input setter with string literal key 'on-setter'
        (
            r"
            @Component()
            class Test {
                @Input('setter') set 'on-setter'(_v: any) {}
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
        // @Input setter with template literal alias `onSetter`
        (
            r"
            @Directive()
            class Test {
                @Input(`onSetter`) set setter(_v: any) {}
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["on"] }])),
        ),
    ];

    Tester::new(NoInputPrefix::NAME, NoInputPrefix::PLUGIN, pass, fail).test_and_snapshot();
}
