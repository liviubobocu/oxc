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
        AngularDecoratorType, get_class_angular_decorator, get_decorator_name
}
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

        // Check if this is an @Input decorator
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Input" {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Find the parent class to verify it's an Angular component/directive
        let Some(class) = get_parent_class(node, ctx) else {
            return;
        };

        let Some((decorator_type, _)) = get_class_angular_decorator(class, ctx) else {
            return;
        };

        if !matches!(
            decorator_type,
            AngularDecoratorType::Component | AngularDecoratorType::Directive
        ) {
            return;
        }

        // Get the property name and its span
        let Some((input_name, property_span)) = get_decorated_property_name_with_span(node, ctx) else {
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
}

fn get_parent_class<'a, 'b>(
    node: &'b AstNode<'a>,
    ctx: &'b LintContext<'a>,
) -> Option<&'b oxc_ast::ast::Class<'a>> {
    for ancestor in ctx.nodes().ancestors(node.id()) {
        if let AstKind::Class(class) = ancestor.kind() {
            return Some(class);
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
        AstKind::PropertyDefinition(prop) => {
            if let oxc_ast::ast::PropertyKey::StaticIdentifier(ident) = &prop.key {
                return Some((ident.name.to_string(), ident.span));
            }
        }
        AstKind::AccessorProperty(prop) => {
            if let oxc_ast::ast::PropertyKey::StaticIdentifier(ident) = &prop.key {
                return Some((ident.name.to_string(), ident.span));
            }
        }
        _ => {}
    }
    None
}

fn get_input_alias_with_span(decorator: &oxc_ast::ast::Decorator<'_>) -> Option<(String, Span)> {
    let call_expr = match &decorator.expression {
        oxc_ast::ast::Expression::CallExpression(call) => call,
        _ => return None
    };

    // @Input('alias') or @Input({ alias: 'alias' })
    let first_arg = call_expr.arguments.first()?;

    match first_arg {
        oxc_ast::ast::Argument::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        oxc_ast::ast::Argument::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop
                    && let oxc_ast::ast::PropertyKey::StaticIdentifier(key) = &obj_prop.key
                        && key.name == "alias"
                            && let oxc_ast::ast::Expression::StringLiteral(lit) = &obj_prop.value {
                                return Some((lit.value.to_string(), lit.span));
                            }
            }
            None
        }
        _ => None
    }
}

fn starts_with_prefix(name: &str, prefix: &str) -> bool {
    if name.len() <= prefix.len() {
        return false;
    }

    if !name.starts_with(prefix) {
        return false;
    }

    // Check that the character after the prefix is uppercase (camelCase)
    name.chars().nth(prefix.len()).is_some_and(|c| c.is_ascii_uppercase())
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
            None,
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
            None,
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
        // Non-Angular class
        (
            r"
            import { Input } from 'other-library';
            class TestClass {
                @Input() onSelect: any;
            }
            ",
            None,
        ),
    ];

    let fail = vec![
        // Input with 'on' prefix (default config)
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
            None,
        ),
        // Input with 'on' prefix via alias
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
            None,
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
            None,
        ),
    ];

    Tester::new(NoInputPrefix::NAME, NoInputPrefix::PLUGIN, pass, fail).test_and_snapshot();
}
