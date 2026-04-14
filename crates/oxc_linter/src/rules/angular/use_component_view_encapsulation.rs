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

fn use_component_view_encapsulation_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Using `ViewEncapsulation.None` makes your styles global, which may have an unintended effect",
    )
    .with_help("Remove `ViewEncapsulation.None`")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseComponentViewEncapsulation;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the use of `ViewEncapsulation.None` in Angular components.
    ///
    /// ### Why is this bad?
    ///
    /// Using `ViewEncapsulation.None` removes Angular's style encapsulation, causing
    /// all component styles to become global. This can lead to:
    /// - Unintended style conflicts with other components
    /// - Difficulty maintaining and debugging styles
    /// - CSS specificity issues
    /// - Styles leaking into or out of components
    ///
    /// Use `ViewEncapsulation.Emulated` (the default) or `ViewEncapsulation.ShadowDom`
    /// to keep styles scoped to the component.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component, ViewEncapsulation } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: '',
    ///   encapsulation: ViewEncapsulation.None
    /// })
    /// export class ExampleComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, ViewEncapsulation } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    ///   // Using default Emulated encapsulation
    /// })
    /// export class ExampleComponent {}
    ///
    /// @Component({
    ///   selector: 'app-shadow',
    ///   template: '',
    ///   encapsulation: ViewEncapsulation.ShadowDom
    /// })
    /// export class ShadowComponent {}
    /// ```
    UseComponentViewEncapsulation,
    angular,
    pedantic,
    pending
);

impl Rule for UseComponentViewEncapsulation {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Component decorator (by name only, matching ESLint behavior)
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

        // Check if encapsulation is set to ViewEncapsulation.None
        // get_metadata_property supports all key types (identifier, string, computed)
        let Some(encapsulation) = get_metadata_property(metadata, "encapsulation") else {
            return;
        };

        // Only report on ViewEncapsulation.None member expression, not numeric literals
        // ESLint specifically matches: MemberExpression[object.name='ViewEncapsulation'] > Identifier[name='None']
        if let Some(none_span) = get_view_encapsulation_none_span(encapsulation) {
            ctx.diagnostic(use_component_view_encapsulation_diagnostic(none_span));
        }
    }
}

/// Check if the expression is `ViewEncapsulation.None` and return the span of the `None` identifier.
/// Returns None if it's not a ViewEncapsulation.None expression.
/// ESLint reports only on the `None` identifier, not the entire `ViewEncapsulation.None` expression.
fn get_view_encapsulation_none_span(expr: &Expression<'_>) -> Option<Span> {
    // Only match ViewEncapsulation.None member expression
    // ESLint selector: MemberExpression[object.name='ViewEncapsulation'] > Identifier[name='None']
    if let Expression::StaticMemberExpression(member) = expr {
        if let Expression::Identifier(obj) = &member.object {
            if obj.name.as_str() == "ViewEncapsulation" && member.property.name.as_str() == "None" {
                // Return the span of just the "None" identifier to match ESLint
                return Some(member.property.span);
            }
        }
    }
    None
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Emulated encapsulation
        r"
    @Component({
      encapsulation: ViewEncapsulation.Emulated,
      selector: 'app-foo-bar'
    })
    class Test {}
  ",
        // Native encapsulation (string literal key)
        r"
    @Component({
      'encapsulation': ViewEncapsulation.Native,
      selector: 'app-foo-bar',
    })
    class Test {}
  ",
        // ShadowDom encapsulation (computed string literal key)
        r"
    @Component({
      ['encapsulation']: ViewEncapsulation.ShadowDom,
    })
    class Test {}
  ",
        // Computed template literal key with function call value (not ViewEncapsulation.None)
        r"
    function encapsulation() {
      return ViewEncapsulation.None;
    }

    @Component({
      [`encapsulation`]: encapsulation()
    })
    class Test {}
  ",
        // Computed identifier key (different variable)
        r"
    const encapsulation = 'templateUrl';
    @Component({
      [encapsulation]: '../a.html'
    })
    class Test {}
  ",
        // Shorthand property (value is variable reference, not ViewEncapsulation.None)
        r"
    const encapsulation = 'templateUrl';
    @Component({
      encapsulation
    })
    class Test {}
  ",
        // Variable reference value
        r"
    const test = 'test';
    @Component({
      encapsulation: test,
    })
    class Test {}
  ",
        // Undefined value
        r"
    @Component({
      encapsulation: undefined,
    })
    class Test {}
  ",
        // Empty component
        r"
    @Component({})
    class Test {}
  ",
        // Variable as decorator argument
        r"
    const options = {};
    @Component(options)
    class Test {}
  ",
        // NgModule (not Component)
        r"
    @NgModule({
      bootstrap: [Foo]
    })
    class Test {}
  ",
    ];

    let fail = vec![
        // Standard identifier key with ViewEncapsulation.None
        r"
      @Component({
        encapsulation: ViewEncapsulation.None,
        selector: 'app-foo-bar',
      })
      class Test {}
    ",
        // String literal key with ViewEncapsulation.None
        r"
      import type { ViewEncapsulation } from '@angular/core';
      import { HttpClient } from '@angular/common/http';

      @Component({
        selector: 'app-foo-bar',
        'encapsulation': ViewEncapsulation.None
      })
      class Test {}
    ",
        // Computed string literal key with ViewEncapsulation.None
        r"
      import { ViewEncapsulation } from '@angular/core';
      import { HttpClient } from '@angular/common/http';

      @Component({
        selector: 'app-foo-bar',
        ['encapsulation']: ViewEncapsulation.None
      })
      class Test {}
    ",
        // Computed template literal key with ViewEncapsulation.None
        r"
      import { ViewEncapsulation } from '@angular/core';
      import { HttpClient } from '@angular/common/http';

      @Component({
        selector: 'app-foo-bar',
        [`encapsulation`]: ViewEncapsulation.None
      })
      class Test {}
    ",
    ];

    Tester::new(
        UseComponentViewEncapsulation::NAME,
        UseComponentViewEncapsulation::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
