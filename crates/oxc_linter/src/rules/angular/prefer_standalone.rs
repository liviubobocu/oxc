use oxc_ast::{AstKind, ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey}};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        get_component_metadata, get_decorator_name,
        get_metadata_property
}
};

fn standalone_false_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Component should use standalone architecture")
        .with_help(
            "Remove `standalone: false` to use standalone components. \
            In Angular 20, components are standalone by default. \
            See https://angular.dev/guide/components/importing for migration guidance.",
        )
        .with_label(span)
}

fn standalone_redundant_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Redundant `standalone: true` property")
        .with_help(
            "In Angular 20, components are standalone by default. \
            You can remove the explicit `standalone: true` declaration.",
        )
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PreferStandalone {
    /// Whether to warn on redundant `standalone: true` (default: false)
    warn_on_redundant: bool
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces standalone component architecture by flagging components that use
    /// `standalone: false` and optionally warning about redundant `standalone: true`.
    ///
    /// ### Why is this bad?
    ///
    /// In Angular 20, standalone components are the default and recommended approach.
    /// Using `standalone: false` requires NgModules which adds complexity and is considered
    /// a legacy pattern.
    ///
    /// The `standalone: true` declaration is redundant in Angular 20 since it's the default,
    /// though this warning is optional and disabled by default.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// // Error: standalone: false is discouraged
    /// @Component({
    ///   selector: 'app-legacy',
    ///   template: '',
    ///   standalone: false
    /// })
    /// export class LegacyComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// // Good: No standalone property (defaults to true in Angular 20)
    /// @Component({
    ///   selector: 'app-modern',
    ///   template: ''
    /// })
    /// export class ModernComponent {}
    /// ```
    ///
    /// ### Options
    ///
    /// ```json
    /// {
    ///   "angular/prefer-standalone": ["error", { "warnOnRedundant": true }]
    /// }
    /// ```
    ///
    /// - `warnOnRedundant`: When `true`, warns about explicit `standalone: true` declarations
    ///   which are redundant in Angular 20. Default is `false`.
    PreferStandalone,
    angular,
    correctness,
    pending // not yet ready for production
);

impl Rule for PreferStandalone {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Component, @Directive, and @Pipe decorators
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Component" && decorator_name != "Directive" && decorator_name != "Pipe" {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Look for the standalone property
        let Some(standalone_value) = get_metadata_property(metadata, "standalone") else {
            // No standalone property - this is fine (defaults to true in Angular 20)
            return;
        };

        // Check the value
        if let Expression::BooleanLiteral(bool_lit) = standalone_value {
            // Get the full property span (key + value) to match ESLint behavior
            let span = find_property_span(metadata, "standalone").unwrap_or(bool_lit.span);

            if !bool_lit.value {
                // standalone: false - this is an error
                ctx.diagnostic(standalone_false_diagnostic(span));
            } else if self.warn_on_redundant {
                // standalone: true - this is redundant (optional warning)
                ctx.diagnostic(standalone_redundant_diagnostic(span));
            }
        }
        // Non-boolean value (e.g., a variable) - we can't statically analyze this
    }
}

/// Find the span of a property in an object expression (includes key and value).
fn find_property_span(obj: &ObjectExpression<'_>, key: &str) -> Option<Span> {
    for property in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = property {
            let key_matches = match &prop.key {
                // Standard identifier key: `standalone: false`
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str() == key,
                // String literal key: `'standalone': false`
                PropertyKey::StringLiteral(lit) => lit.value.as_str() == key,
                _ => {
                    // For computed keys, check the expression
                    if prop.computed {
                        match prop.key.as_expression() {
                            // Computed string literal: `['standalone']: false`
                            Some(Expression::StringLiteral(lit)) => lit.value.as_str() == key,
                            // Computed template literal with no expressions: `` [`standalone`]: false ``
                            Some(Expression::TemplateLiteral(tpl))
                                if tpl.expressions.is_empty() =>
                            {
                                tpl.quasis.first().is_some_and(|q| q.value.raw.as_str() == key)
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
            };

            if key_matches {
                return Some(prop.span());
            }
        }
    }
    None
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // No standalone property (defaults to true in Angular 20)
        r"
        import { Component } from '@angular/core';
        @Component({ selector: 'app-test', template: '' })
        class TestComponent {}
        ",
        // Directive without standalone property
        r"
        import { Directive } from '@angular/core';
        @Directive({ selector: '[appTest]' })
        class TestDirective {}
        ",
        // standalone: true without warnOnRedundant option (default)
        r"
        import { Component } from '@angular/core';
        @Component({ selector: 'app-test', template: '', standalone: true })
        class TestComponent {}
        ",
        // Directive with standalone: true (default option, no warning)
        r"
        import { Directive } from '@angular/core';
        @Directive({ selector: '[appTest]', standalone: true })
        class TestDirective {}
        ",
        // Injectable (not Component/Directive)
        r"
        import { Injectable } from '@angular/core';
        @Injectable({ providedIn: 'root' })
        class TestService {}
        ",
        // Pipe with standalone: true (no warning by default)
        r"
        import { Pipe } from '@angular/core';
        @Pipe({ name: 'test', standalone: true })
        class TestPipe {}
        ",
        // Pipe without standalone property (defaults to true in Angular 20)
        r"
        import { Pipe } from '@angular/core';
        @Pipe({ name: 'test' })
        class TestPipe {}
        ",
        // Component with templateUrl and no standalone
        r"
        import { Component } from '@angular/core';
        @Component({ selector: 'app-test', templateUrl: './test.html' })
        class TestComponent {}
        ",
        // Component with imports array (implicit standalone)
        r"
        import { Component } from '@angular/core';
        import { CommonModule } from '@angular/common';
        @Component({ selector: 'app-test', template: '', imports: [CommonModule] })
        class TestComponent {}
        ",
    ];

    let fail = vec![
        // standalone: false on Component
        r"
        import { Component } from '@angular/core';
        @Component({ selector: 'app-test', template: '', standalone: false })
        class TestComponent {}
        ",
        // standalone: false on Directive
        r"
        import { Directive } from '@angular/core';
        @Directive({ selector: '[appTest]', standalone: false })
        class TestDirective {}
        ",
        // standalone: false with other metadata
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '<div>Test</div>',
            styleUrls: ['./test.css'],
            standalone: false
        })
        class TestComponent {}
        ",
        // standalone: false on Component with providers
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: '',
            providers: [],
            standalone: false
        })
        class TestComponent {}
        ",
        // standalone: false on attribute directive
        r"
        import { Directive } from '@angular/core';
        @Directive({
            selector: '[appHighlight]',
            standalone: false
        })
        class HighlightDirective {}
        ",
        // standalone: false on Pipe
        r"
        import { Pipe } from '@angular/core';
        @Pipe({
            name: 'testPipe',
            standalone: false
        })
        class TestPipe {}
        ",
        // standalone: false on Pipe with other metadata
        r"
        import { Pipe } from '@angular/core';
        @Pipe({
            name: 'testPipe',
            pure: true,
            standalone: false
        })
        class TestPipe {}
        ",
    ];

    Tester::new(PreferStandalone::NAME, PreferStandalone::PLUGIN, pass, fail).test_and_snapshot();
}

#[test]
fn test_warn_on_redundant() {
    use crate::tester::Tester;
    use serde_json::json;

    // When warnOnRedundant is true, standalone: true should trigger a warning
    let pass: Vec<(&str, Option<serde_json::Value>)> = vec![];

    let fail = vec![(
        r"
        import { Component } from '@angular/core';
        @Component({ selector: 'app-test', template: '', standalone: true })
        class TestComponent {}
        ",
        Some(json!([{ "warnOnRedundant": true }])),
    )];

    Tester::new(PreferStandalone::NAME, PreferStandalone::PLUGIN, pass, fail)
        .with_snapshot_suffix("warn_on_redundant")
        .test_and_snapshot();
}
