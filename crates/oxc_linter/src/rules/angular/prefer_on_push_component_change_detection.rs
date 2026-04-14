use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_call, get_decorator_name,
        get_metadata_property
}
};

fn prefer_on_push_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "The component's `changeDetection` value should be set to `ChangeDetectionStrategy.OnPush`",
    )
    .with_help(
        "Add `changeDetection: ChangeDetectionStrategy.OnPush` to the component decorator.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferOnPushComponentChangeDetection;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the use of `ChangeDetectionStrategy.OnPush` in Angular components.
    ///
    /// ### Why is this bad?
    ///
    /// Using `ChangeDetectionStrategy.Default` (the default) can lead to performance issues
    /// because Angular will check the component and its children on every change detection cycle.
    ///
    /// `ChangeDetectionStrategy.OnPush` provides better performance by:
    /// - Only checking when input references change
    /// - Only checking when events are triggered within the component
    /// - Only checking when explicitly triggered via `markForCheck()` or `detectChanges()`
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, ChangeDetectionStrategy } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: '',
    ///   changeDetection: ChangeDetectionStrategy.OnPush
    /// })
    /// export class ExampleComponent {}
    /// ```
    PreferOnPushComponentChangeDetection,
    angular,
    pedantic,
    pending
);

impl Rule for PreferOnPushComponentChangeDetection {
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

        // Get the call expression to check for arguments
        let Some(call) = get_decorator_call(decorator) else {
            return;
        };

        // Check if @Component() has no arguments - report on the decorator
        if call.arguments.is_empty() {
            ctx.diagnostic(prefer_on_push_diagnostic(decorator.span));
            return;
        }

        // Get the metadata object (first argument must be an object expression)
        let Some(metadata) = get_component_metadata(decorator) else {
            // First argument is not an object (e.g., @Component(options))
            // ESLint does not report on this case
            return;
        };

        // Check if changeDetection is set to OnPush
        match get_metadata_property(metadata, "changeDetection") {
            None => {
                // No changeDetection property - using default (report on decorator)
                ctx.diagnostic(prefer_on_push_diagnostic(decorator.span));
            }
            Some(expr) => {
                // Check if it's a problematic value
                if let Some(report_span) = get_invalid_change_detection_span(expr) {
                    ctx.diagnostic(prefer_on_push_diagnostic(report_span));
                }
                // Note: ESLint does NOT report on variable references, function calls,
                // or other expressions - only on explicit ChangeDetectionStrategy.X
                // where X != OnPush, or on `undefined`
            }
        }
    }
}

/// Returns the span to report on if the changeDetection value is invalid,
/// or None if the value is valid or cannot be statically analyzed.
///
/// ESLint only reports errors for:
/// 1. `changeDetection: undefined` - reports on the `undefined` identifier
/// 2. `changeDetection: ChangeDetectionStrategy.X` where X != 'OnPush' - reports on the property name (X)
///
/// ESLint does NOT report for:
/// - Variable references (e.g., `changeDetection: someVariable`)
/// - Function calls (e.g., `changeDetection: getStrategy()`)
/// - Other expressions that cannot be statically analyzed
fn get_invalid_change_detection_span(expr: &Expression<'_>) -> Option<Span> {
    match expr {
        // Check for ChangeDetectionStrategy.X
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name.as_str() == "ChangeDetectionStrategy" {
                    // Only report if it's NOT OnPush
                    if member.property.name.as_str() != "OnPush" {
                        // Report on the property name (e.g., "Default")
                        return Some(member.property.span);
                    }
                }
            }
            // Not ChangeDetectionStrategy.X, don't report
            None
        }
        // Check for `undefined` identifier
        Expression::Identifier(ident) => {
            if ident.name.as_str() == "undefined" {
                Some(ident.span)
            } else {
                // Variable reference - don't report (ESLint allows this)
                None
            }
        }
        // Any other expression (function call, etc.) - don't report
        _ => None,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // No class - just a plain class without decorator
        r"class Test {}",
        // @Component with options variable (not an object literal)
        r"
        const options = {};
        @Component(options)
        class Test {}
        ",
        // OnPush change detection
        r"
        @Component({
            changeDetection: ChangeDetectionStrategy.OnPush,
        })
        class Test {}
        ",
        // changeDetection with variable reference (ESLint allows this)
        r"
        @Component({
            'changeDetection': changeDetection,
        })
        class Test {}
        ",
        // Shorthand property syntax (ESLint allows this)
        r"
        const changeDetection = ChangeDetectionStrategy.Default;
        @Component({
            changeDetection,
        })
        class Test {}
        ",
        // Function call value (ESLint allows this)
        r"
        function changeDetection() {
            return ChangeDetectionStrategy.OnPush;
        }

        @Component({
            ['changeDetection']: changeDetection(),
        })
        class Test {}
        ",
        // Template literal key with OnPush
        r"
        @Component({
            [`changeDetection`]: ChangeDetectionStrategy.OnPush,
        })
        class Test {}
        ",
        // NgModule (not a component)
        r"
        @NgModule({
            bootstrap: [Foo]
        })
        class Test {}
        ",
    ];

    let fail = vec![
        // @Component() with no arguments
        r"
      @Component()

      class Test {}
    ",
        // @Component({}) with empty object
        r"
      import type { ChangeDetectionStrategy } from '@angular/core';

      @Component({})

      class Test {}
    ",
        // @Component with computed key but no changeDetection
        r"
      import { Component } from '@angular/core';
      const changeDetection = 'template';
      @Component({ [changeDetection]: '' })

      class Test {}
    ",
        // changeDetection: undefined
        r"
      @Component({ changeDetection: undefined })

      class Test {}
    ",
        // String literal key with ChangeDetectionStrategy.Default
        r"
      import * as ng from '@angular/core';
      @Component({ 'changeDetection': ChangeDetectionStrategy.Default })

      class Test {}
    ",
        // Computed string literal key with ChangeDetectionStrategy.Default
        r"
      import type { OnInit } from '@angular/core';
      @Component({ ['changeDetection']: ChangeDetectionStrategy.Default })

      class Test {}
    ",
        // Computed template literal key with ChangeDetectionStrategy.Default
        r"
      import ng from '@angular/core';
      @Component({ [`changeDetection`]: ChangeDetectionStrategy.Default })

      class Test {}
    ",
    ];

    Tester::new(
        PreferOnPushComponentChangeDetection::NAME,
        PreferOnPushComponentChangeDetection::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
