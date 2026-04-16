use oxc_ast::AstKind;
use oxc_ast::ast::Expression;
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
        get_component_metadata, get_decorator_name
}
};

fn consistent_component_styles_diagnostic(span: Span, message_id: &str) -> OxcDiagnostic {
    // Match ESLint message format exactly
    let (message, help) = match message_id {
        "useStylesString" => (
            "Use a `string` instead of a `string[]` for the `styles` property",
            "Replace the array with a single string value",
        ),
        "useStylesArray" => (
            "Use a `string[]` instead of a `string` for the `styles` property",
            "Wrap the string in an array: `styles: ['css-here']`",
        ),
        "useStyleUrl" => (
            "Use `styleUrl` instead of `styleUrls` for a single stylesheet",
            "Use `styleUrl: './style.css'` instead of `styleUrls: ['./style.css']`",
        ),
        "useStyleUrls" => (
            "Use `styleUrls` instead of `styleUrl`",
            "Use `styleUrls: ['./style.css']` instead of `styleUrl: './style.css'`",
        ),
        _ => ("Use consistent style format", "Use consistent style format")
    };
    OxcDiagnostic::warn(message).with_help(help).with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum StyleFormat {
    #[default]
    String,
    Array
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ConsistentComponentStylesConfig {
    /// The preferred format for styles: "string" or "array"
    #[serde(default)]
    format: StyleFormat
}

#[derive(Debug, Clone)]
pub struct ConsistentComponentStyles {
    format: StyleFormat
}

impl Default for ConsistentComponentStyles {
    fn default() -> Self {
        Self { format: StyleFormat::String }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces consistent usage of `styles` property format in components.
    ///
    /// ### Why is this bad?
    ///
    /// Angular allows both string and array formats for inline styles:
    /// - `styles: 'css-here'` (string format)
    /// - `styles: ['css-here']` (array format)
    ///
    /// Using a consistent format across your codebase improves readability and
    /// makes it easier to understand the component structure at a glance.
    ///
    /// Note: Since Angular 17, the preferred approach is to use `styleUrl` (singular)
    /// for a single file or `styleUrls` for multiple files. This rule focuses on
    /// the inline `styles` property.
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/consistent-component-styles": ["error", { "format": "string" }]
    /// }
    /// ```
    ///
    /// Options:
    /// - `"string"` (default): Enforce single string format
    /// - `"array"`: Enforce array format
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with `"string"` config):
    /// ```typescript
    /// @Component({
    ///   selector: 'app-example',
    ///   template: '',
    ///   styles: ['.class { color: red; }'] // Should be a string
    /// })
    /// export class ExampleComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule (with `"string"` config):
    /// ```typescript
    /// @Component({
    ///   selector: 'app-example',
    ///   template: '',
    ///   styles: '.class { color: red; }'
    /// })
    /// export class ExampleComponent {}
    /// ```
    ConsistentComponentStyles,
    angular,
    style,
    pending,
    config = ConsistentComponentStylesConfig
);

impl Rule for ConsistentComponentStyles {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);

        // Handle string shorthand: ["string"] or ["array"]
        if let Some(format_str) = config_value.as_str() {
            let format = match format_str {
                "array" => StyleFormat::Array,
                _ => StyleFormat::String
};
            return Ok(Self { format });
        }

        let config: ConsistentComponentStylesConfig = serde_json::from_value(config_value.clone())?;
        Ok(Self { format: config.format })
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
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Find style-related properties
        for prop in &metadata.properties {
            if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop {
                let prop_name = match &obj_prop.key {
                    oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
                    oxc_ast::ast::PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
                    _ => None
};

                match prop_name {
                    Some("styles") => {
                        match &self.format {
                            StyleFormat::String => {
                                // In string mode, report single-element arrays that contain a literal/template
                                if let Expression::ArrayExpression(array) = &obj_prop.value {
                                    // Only report if it's a single-element array with a string/template literal
                                    if array.elements.len() == 1 {
                                        if let Some(first_element) = array.elements.first() {
                                            if let Some(expr) = first_element.as_expression() {
                                                // Check if element is a string literal or template literal
                                                if matches!(expr, Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) {
                                                    // Report on the array value (matching ESLint)
                                                    ctx.diagnostic(consistent_component_styles_diagnostic(
                                                        obj_prop.value.span(),
                                                        "useStylesString",
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            StyleFormat::Array => {
                                // In array mode, report string literals and template literals
                                // Only flag literal values, not complex expressions
                                if matches!(&obj_prop.value, Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) {
                                    // Report on the string/template value (matching ESLint)
                                    ctx.diagnostic(consistent_component_styles_diagnostic(
                                        obj_prop.value.span(),
                                        "useStylesArray",
                                    ));
                                }
                            }
                        }
                    }
                    Some("styleUrl") => {
                        // styleUrl is singular - in array mode, we expect styleUrls
                        // Only report if the value is a string literal or template element
                        if matches!(self.format, StyleFormat::Array) {
                            if matches!(&obj_prop.value, Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) {
                                ctx.diagnostic(consistent_component_styles_diagnostic(
                                    obj_prop.span,
                                    "useStyleUrls",
                                ));
                            }
                        }
                    }
                    Some("styleUrls") => {
                        // styleUrls is plural - in string mode with single element, we expect styleUrl
                        if matches!(self.format, StyleFormat::String) {
                            if let Expression::ArrayExpression(array) = &obj_prop.value {
                                if array.elements.len() == 1 {
                                    // Check if the single element is a string literal or template literal
                                    if let Some(first_element) = array.elements.first() {
                                        if let Some(expr) = first_element.as_expression() {
                                            if matches!(expr, Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) {
                                                ctx.diagnostic(consistent_component_styles_diagnostic(
                                                    obj_prop.span,
                                                    "useStyleUrl",
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // String format with string config (default)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: '.class { color: red; }'
            })
            class TestComponent {}
            ",
            None,
        ),
        // No styles property
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: ''
            })
            class TestComponent {}
            ",
            None,
        ),
        // Array format with array config (object format)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: ['.class { color: red; }']
            })
            class TestComponent {}
            ",
            Some(serde_json::json!([{ "format": "array" }])),
        ),
        // Array format with array config (shorthand format)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: ['.class { color: red; }']
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // Multiple styles in array (always allowed)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: ['.class1 { color: red; }', '.class2 { color: blue; }']
            })
            class TestComponent {}
            ",
            None,
        ),
        // Template literal string
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: `.class { color: red; }`
            })
            class TestComponent {}
            ",
            None,
        ),
        // Template literal string with explicit "string" config
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: `.class { color: red; }`
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["string"])),
        ),
        // styleUrl with string config (default) - singular is fine
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrl: './test.component.css'
            })
            class TestComponent {}
            ",
            None,
        ),
        // styleUrls with array config (shorthand)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrls: ['./test.component.css']
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // Multiple styleUrls (always allowed with string config)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrls: ['./test1.css', './test2.css']
            })
            class TestComponent {}
            ",
            None,
        ),
        // styleUrl with template literal - valid in string mode
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrl: `./test.component.css`
            })
            class TestComponent {}
            ",
            None,
        ),
    ];

    let fail = vec![
        // Single-element array with string config (default)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: ['.class { color: red; }']
            })
            class TestComponent {}
            ",
            None,
        ),
        // Single-element array with explicit "string" config (shorthand)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: ['.class { color: red; }']
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["string"])),
        ),
        // String format with array config (object format)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: '.class { color: red; }'
            })
            class TestComponent {}
            ",
            Some(serde_json::json!([{ "format": "array" }])),
        ),
        // String format with array config (shorthand format)
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: '.class { color: red; }'
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // Template literal in single-element array
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: [`.class { color: red; }`]
            })
            class TestComponent {}
            ",
            None,
        ),
        // Template literal string with array config
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styles: `.class { color: red; }`
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // Single-element styleUrls with string config - should use styleUrl
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrls: ['./test.component.css']
            })
            class TestComponent {}
            ",
            None,
        ),
        // Single-element styleUrls with explicit "string" config
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrls: ['./test.component.css']
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["string"])),
        ),
        // styleUrl with array config - should use styleUrls
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrl: './test.component.css'
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // styleUrl with template literal and array config
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrl: `./test.component.css`
            })
            class TestComponent {}
            ",
            Some(serde_json::json!(["array"])),
        ),
        // styleUrls with single template literal - should use styleUrl
        (
            r"
            import { Component } from '@angular/core';
            @Component({
                selector: 'app-test',
                template: '',
                styleUrls: [`./test.component.css`]
            })
            class TestComponent {}
            ",
            None,
        ),
    ];

    Tester::new(ConsistentComponentStyles::NAME, ConsistentComponentStyles::PLUGIN, pass, fail)
        .test_and_snapshot();
}
