use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_name,
        get_metadata_property
}
};

fn use_component_selector_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Components should have a selector")
        .with_help(
            "Add a `selector` property to the @Component decorator. Without a selector, \
            the component can only be used programmatically and not in templates.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseComponentSelector;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that all `@Component` decorators have a `selector` property defined.
    ///
    /// ### Why is this bad?
    ///
    /// A component without a selector cannot be used in templates and can only be
    /// created programmatically. While this might be intentional for some components
    /// (like those used with route configurations), most components should have a
    /// selector defined.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// @Component({
    ///   template: '<div>Hello</div>'
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
    ///   template: '<div>Hello</div>'
    /// })
    /// export class ExampleComponent {}
    /// ```
    UseComponentSelector,
    angular,
    pedantic,
    pending
);

impl Rule for UseComponentSelector {
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
            // No metadata object means @Component() with no arguments - invalid
            ctx.diagnostic(use_component_selector_diagnostic(decorator.span));
            return;
        };

        // Check if selector is present and valid
        // ESLint accepts: non-empty string literals and template literals
        match get_metadata_property(metadata, "selector") {
            None => {
                // No selector property at all
                ctx.diagnostic(use_component_selector_diagnostic(decorator.span));
            }
            Some(Expression::StringLiteral(lit)) => {
                // Empty string literal is invalid
                if lit.value.is_empty() {
                    ctx.diagnostic(use_component_selector_diagnostic(decorator.span));
                }
                // Non-empty string literal is valid
            }
            Some(Expression::TemplateLiteral(_)) => {
                // Template literals are always valid (ESLint behavior)
            }
            _ => {
                // All other expression types are invalid (including identifiers)
                ctx.diagnostic(use_component_selector_diagnostic(decorator.span));
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Component with selector
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {}
        ",
        // Component with attribute selector
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: '[appTest]',
            template: ''
        })
        class TestComponent {}
        ",
        // Component with template literal selector
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: `app-test`,
            template: ''
        })
        class TestComponent {}
        ",
        // Standalone component with selector
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            standalone: true
        })
        class TestComponent {}
        ",
        // Non-Angular Component
        r"
        import { Component } from 'other-lib';
        @Component({
            template: ''
        })
        class TestComponent {}
        ",
        // Directive (not a component)
        r"
        import { Directive } from '@angular/core';
        @Directive({
            selector: '[appTest]'
        })
        class TestDirective {}
        ",
    ];

    let fail = vec![
        // Component without selector
        r"
        import { Component } from '@angular/core';
        @Component({
            template: ''
        })
        class TestComponent {}
        ",
        // Component with empty selector
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: '',
            template: ''
        })
        class TestComponent {}
        ",
        // Standalone component without selector
        r"
        import { Component } from '@angular/core';
        @Component({
            template: '',
            standalone: true
        })
        class TestComponent {}
        ",
        // Component with templateUrl but no selector
        r"
        import { Component } from '@angular/core';
        @Component({
            templateUrl: './test.component.html'
        })
        class TestComponent {}
        ",
        // Component with identifier reference (shorthand) - ESLint rejects this
        r"
        import { Component } from '@angular/core';
        const selector = 'app-test';
        @Component({
            selector,
            template: ''
        })
        class TestComponent {}
        ",
        // Component with identifier reference (longhand) - ESLint rejects this
        r"
        import { Component } from '@angular/core';
        const selectorVar = 'app-test';
        @Component({
            selector: selectorVar,
            template: ''
        })
        class TestComponent {}
        ",
    ];

    Tester::new(UseComponentSelector::NAME, UseComponentSelector::PLUGIN, pass, fail)
        .test_and_snapshot();
}
