use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{get_component_metadata, get_decorator_name, get_metadata_property},
};

fn relative_url_prefix_diagnostic(span: Span, property: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "`{property}` should use relative paths starting with `./` or `../`"
    ))
    .with_help(
        "Use relative paths (starting with `./` or `../`) for `templateUrl` and `styleUrls` \
        to ensure proper resolution in all build scenarios.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RelativeUrlPrefix;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `templateUrl` and `styleUrls` in `@Component` decorators use relative
    /// paths starting with `./` or `../`.
    ///
    /// ### Why is this bad?
    ///
    /// Using non-relative paths for external templates and styles can cause:
    /// - Build failures in certain configurations
    /// - Inconsistent behavior across different bundlers
    /// - Issues with Angular CLI and component compilation
    ///
    /// The standard syntax for relative URLs requires paths to begin with `./` or `../`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   templateUrl: 'example.component.html',
    ///   styleUrls: ['example.component.css']
    /// })
    /// export class ExampleComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   templateUrl: './example.component.html',
    ///   styleUrls: ['./example.component.css']
    /// })
    /// export class ExampleComponent {}
    /// ```
    RelativeUrlPrefix,
    angular,
    pedantic,
    pending
);

impl Rule for RelativeUrlPrefix {
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

        // Note: ESLint's selector only matches decorator name, not import source
        // (COMPONENT_CLASS_DECORATOR = 'ClassDeclaration > Decorator[expression.callee.name="Component"]')
        // So we match this lenient behavior for exact parity

        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Check templateUrl
        // Latest ESLint supports template literals: https://github.com/angular-eslint/angular-eslint/issues/2575
        if let Some(template_url) = get_metadata_property(metadata, "templateUrl") {
            if let Some((path, span)) = extract_url_value(template_url) {
                if !is_relative_path(&path) {
                    ctx.diagnostic(relative_url_prefix_diagnostic(span, "templateUrl"));
                }
            }
        }

        // Check styleUrls (array of strings)
        // Latest ESLint supports template literals: https://github.com/angular-eslint/angular-eslint/issues/2575
        if let Some(style_urls) = get_metadata_property(metadata, "styleUrls") {
            if let Expression::ArrayExpression(arr) = style_urls {
                for element in &arr.elements {
                    if let Some(expr) = element.as_expression() {
                        if let Some((path, span)) = extract_url_value(expr) {
                            if !is_relative_path(&path) {
                                ctx.diagnostic(relative_url_prefix_diagnostic(span, "styleUrls"));
                            }
                        }
                    }
                }
            }
        }

        // Check styleUrl (single string, Angular 17+)
        // Latest ESLint supports template literals: https://github.com/angular-eslint/angular-eslint/issues/2575
        if let Some(style_url) = get_metadata_property(metadata, "styleUrl") {
            if let Some((path, span)) = extract_url_value(style_url) {
                if !is_relative_path(&path) {
                    ctx.diagnostic(relative_url_prefix_diagnostic(span, "styleUrl"));
                }
            }
        }
    }
}

/// Extract URL value from a string literal or template literal.
/// Returns the path string and span, or None if the expression type is not supported.
///
/// Template literal support was added in angular-eslint PR #2576 (merged July 2025).
/// See: https://github.com/angular-eslint/angular-eslint/issues/2575
fn extract_url_value(expr: &Expression<'_>) -> Option<(String, Span)> {
    match expr {
        Expression::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        Expression::TemplateLiteral(lit) => {
            // Support simple template literals without expressions
            // For template literals, use the raw value of the first quasi (matching ESLint)
            if lit.quasis.len() == 1 && lit.expressions.is_empty() {
                let quasi = &lit.quasis[0];
                Some((quasi.value.raw.to_string(), lit.span))
            } else {
                // Template literal with expressions - can't validate statically
                None
            }
        }
        _ => None,
    }
}

fn is_relative_path(path: &str) -> bool {
    path.starts_with("./") || path.starts_with("../")
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Relative templateUrl
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            templateUrl: './test.component.html'
        })
        class TestComponent {}
        ",
        // Relative styleUrls
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            styleUrls: ['./test.component.css']
        })
        class TestComponent {}
        ",
        // Multiple relative styleUrls
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            styleUrls: ['./test.component.css', '../shared/styles.css']
        })
        class TestComponent {}
        ",
        // Parent directory relative path
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            templateUrl: '../templates/test.component.html'
        })
        class TestComponent {}
        ",
        // Inline template (not affected)
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '<div>Hello</div>'
        })
        class TestComponent {}
        ",
        // Inline styles (not affected)
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            styles: [':host { display: block; }']
        })
        class TestComponent {}
        ",
        // Template literals with valid relative paths (ESLint PR #2576)
        // See: https://github.com/angular-eslint/angular-eslint/issues/2575
        r"
        import { Component } from '@angular/core';
        @Component({
            templateUrl: `../foobar.html`,
            styleUrls: [
                `.././foobar.css`,
            ]
        })
        class TestComponent {}
        ",
        // Note: ESLint's selector only matches by decorator name, not import source
        // So @Component from 'other-lib' is also matched. We match this behavior for parity.
    ];

    let fail = vec![
        // Non-relative templateUrl
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            templateUrl: 'test.component.html'
        })
        class TestComponent {}
        ",
        // Non-relative styleUrls
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            styleUrls: ['test.component.css']
        })
        class TestComponent {}
        ",
        // Absolute path templateUrl
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            templateUrl: '/app/test.component.html'
        })
        class TestComponent {}
        ",
        // Mixed - one relative, one not
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            styleUrls: ['./valid.css', 'invalid.css']
        })
        class TestComponent {}
        ",
        // Template literals with INVALID paths (no relative prefix)
        r"
        import { Component } from '@angular/core';
        @Component({
            templateUrl: `foobar.html`,
            styleUrls: [
                `styles.css`,
            ]
        })
        class TestComponent {}
        ",
    ];

    Tester::new(RelativeUrlPrefix::NAME, RelativeUrlPrefix::PLUGIN, pass, fail).test_and_snapshot();
}
