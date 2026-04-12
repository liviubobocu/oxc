use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{AstNode, context::LintContext, rule::Rule};

fn no_conflicting_lifecycle_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Class implements both `DoCheck` and `OnChanges` lifecycle interfaces")
        .with_help(
            "Implementing both `DoCheck` and `OnChanges` can lead to unexpected behavior. \
            `ngOnChanges` is called when input properties change, while `ngDoCheck` is called \
            during every change detection cycle. Choose one based on your needs: use `ngOnChanges` \
            for reacting to input changes, or `ngDoCheck` for custom change detection logic.",
        )
        .with_label(span)
}

fn no_conflicting_lifecycle_interface_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Implementing both `DoCheck` and `OnChanges` is not recommended")
        .with_help(
            "Implementing both `DoCheck` and `OnChanges` can lead to unexpected behavior. \
            `ngOnChanges` is called when input properties change, while `ngDoCheck` is called \
            during every change detection cycle. Choose one based on your needs: use `ngOnChanges` \
            for reacting to input changes, or `ngDoCheck` for custom change detection logic.",
        )
        .with_label(span)
}

fn no_conflicting_lifecycle_method_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Implementing both `DoCheck` and `OnChanges` is not recommended")
        .with_help(
            "Implementing both `DoCheck` and `OnChanges` can lead to unexpected behavior. \
            `ngOnChanges` is called when input properties change, while `ngDoCheck` is called \
            during every change detection cycle. Choose one based on your needs: use `ngOnChanges` \
            for reacting to input changes, or `ngDoCheck` for custom change detection logic.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoConflictingLifecycle;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows implementing both `DoCheck` and `OnChanges` interfaces in the same class.
    ///
    /// ### Why is this bad?
    ///
    /// The `DoCheck` and `OnChanges` lifecycle hooks serve different purposes and implementing
    /// both can lead to confusing behavior:
    ///
    /// - `ngOnChanges` is called when Angular detects changes to input properties
    /// - `ngDoCheck` is called during every change detection run
    ///
    /// When both are implemented:
    /// - `ngOnChanges` runs first (when inputs change)
    /// - `ngDoCheck` runs immediately after
    /// - This can cause duplicate processing and performance issues
    ///
    /// Note: This rule is marked as deprecated in angular-eslint due to potential false
    /// positives in legitimate use cases, but is included for completeness.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component, DoCheck, OnChanges, SimpleChanges } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent implements DoCheck, OnChanges {
    ///   ngDoCheck() {}
    ///   ngOnChanges(changes: SimpleChanges) {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, OnChanges, SimpleChanges } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent implements OnChanges {
    ///   ngOnChanges(changes: SimpleChanges) {}
    /// }
    /// ```
    NoConflictingLifecycle,
    angular,
    pedantic,
    pending
);

impl Rule for NoConflictingLifecycle {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };

        let mut has_do_check = false;
        let mut has_on_changes = false;
        let mut do_check_interface_spans = Vec::new();
        let mut on_changes_interface_spans = Vec::new();
        let mut do_check_method_spans = Vec::new();
        let mut on_changes_method_spans = Vec::new();

        // Check implemented interfaces
        for ts_impl in &class.implements {
            if let oxc_ast::ast::TSTypeName::IdentifierReference(ident) = &ts_impl.expression {
                match ident.name.as_str() {
                    "DoCheck" => {
                        has_do_check = true;
                        do_check_interface_spans.push(ident.span);
                    }
                    "OnChanges" => {
                        has_on_changes = true;
                        on_changes_interface_spans.push(ident.span);
                    }
                    _ => {}
                }
            }
        }

        // Also check for the presence of both methods (even without explicit implements)
        for element in &class.body.body {
            if let oxc_ast::ast::ClassElement::MethodDefinition(method) = element
                && let Some(name) = method.key.static_name() {
                    match name.as_ref() {
                        "ngDoCheck" => {
                            has_do_check = true;
                            do_check_method_spans.push(method.key.span());
                        }
                        "ngOnChanges" => {
                            has_on_changes = true;
                            on_changes_method_spans.push(method.key.span());
                        }
                        _ => {}
                    }
                }
        }

        if has_do_check && has_on_changes {
            // Report on each interface
            for span in do_check_interface_spans {
                ctx.diagnostic(no_conflicting_lifecycle_interface_diagnostic(span));
            }
            for span in on_changes_interface_spans {
                ctx.diagnostic(no_conflicting_lifecycle_interface_diagnostic(span));
            }

            // Report on each method
            for span in do_check_method_spans {
                ctx.diagnostic(no_conflicting_lifecycle_method_diagnostic(span));
            }
            for span in on_changes_method_spans {
                ctx.diagnostic(no_conflicting_lifecycle_method_diagnostic(span));
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Only OnChanges
        r"
        import { Component, OnChanges, SimpleChanges } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent implements OnChanges {
            ngOnChanges(changes: SimpleChanges) {}
        }
        ",
        // Only DoCheck
        r"
        import { Component, DoCheck } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent implements DoCheck {
            ngDoCheck() {}
        }
        ",
        // Neither DoCheck nor OnChanges
        r"
        import { Component, OnInit } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent implements OnInit {
            ngOnInit() {}
        }
        ",
    ];

    let fail = vec![
        // Both interfaces implemented
        r"
        import { Component, DoCheck, OnChanges, SimpleChanges } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent implements DoCheck, OnChanges {
            ngDoCheck() {}
            ngOnChanges(changes: SimpleChanges) {}
        }
        ",
        // Both methods present (without explicit implements)
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngDoCheck() {}
            ngOnChanges() {}
        }
        ",
        // Directive with conflicting lifecycle
        r"
        import { Directive, DoCheck, OnChanges } from '@angular/core';
        @Directive({
            selector: '[appTest]'
        })
        class TestDirective implements DoCheck, OnChanges {
            ngDoCheck() {}
            ngOnChanges() {}
        }
        ",
    ];

    Tester::new(NoConflictingLifecycle::NAME, NoConflictingLifecycle::PLUGIN, pass, fail)
        .test_and_snapshot();
}
