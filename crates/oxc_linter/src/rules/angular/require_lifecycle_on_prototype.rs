use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{get_class_angular_decorator_lenient, is_lifecycle_method}
};

fn require_lifecycle_on_prototype_diagnostic(span: Span, method_name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Lifecycle method `{method_name}` should be defined on the class prototype, not as a property"
    ))
    .with_help(
        "Define lifecycle methods as regular methods on the class, not as property assignments. \
        Angular's change detection and lifecycle hooks work with prototype methods, not instance properties.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireLifecycleOnPrototype;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures lifecycle methods are declared on the class prototype rather than as instance properties.
    ///
    /// ### Why is this bad?
    ///
    /// Lifecycle methods defined as arrow functions or property assignments create a new function
    /// for each component instance. This has several drawbacks:
    /// - Higher memory usage (each instance has its own copy)
    /// - Cannot be overridden in subclasses
    /// - May not work correctly with Angular's change detection in some scenarios
    /// - Inconsistent with Angular's expected method signature pattern
    ///
    /// Regular prototype methods are shared across all instances and work correctly with
    /// Angular's lifecycle system.
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
    /// export class ExampleComponent {
    ///   ngOnInit = () => {
    ///     console.log('initialized');
    ///   };
    ///
    ///   ngOnDestroy = function() {
    ///     console.log('destroyed');
    ///   };
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   ngOnInit() {
    ///     console.log('initialized');
    ///   }
    ///
    ///   ngOnDestroy() {
    ///     console.log('destroyed');
    ///   }
    /// }
    /// ```
    RequireLifecycleOnPrototype,
    angular,
    correctness
);

impl Rule for RequireLifecycleOnPrototype {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::PropertyDefinition(prop) => {
                check_property_definition(prop, node, ctx);
            }
            AstKind::AssignmentExpression(assignment) => {
                check_assignment_expression(assignment, ctx);
            }
            _ => {}
        }
    }
}

fn check_property_definition<'a>(
    prop: &oxc_ast::ast::PropertyDefinition<'a>,
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) {
    // Get property name - handle both static identifiers and computed properties
    let prop_name = match &prop.key {
        oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
        oxc_ast::ast::PropertyKey::StringLiteral(lit) => lit.value.as_str(),
        _ => return,
    };

    // Check if this is a lifecycle method name
    if !is_lifecycle_method(prop_name) {
        return;
    }

    // ESLint rule checks for ANY value assignment, not just functions
    // This includes: ngOnInit = func, ngOnInit = () => {}, ngOnInit = function() {}
    // All of these are violations - lifecycle methods should be on the prototype
    if prop.value.is_none() {
        return;
    }

    // Find the parent class
    let Some(class) = get_parent_class(node, ctx) else {
        return;
    };

    // Check if the class has an Angular decorator
    if get_class_angular_decorator_lenient(class, ctx).is_none() {
        return;
    }

    ctx.diagnostic(require_lifecycle_on_prototype_diagnostic(prop.key.span(), prop_name));
}

fn check_assignment_expression<'a>(
    assignment: &oxc_ast::ast::AssignmentExpression<'a>,
    ctx: &LintContext<'a>,
) {
    use oxc_ast::ast::{AssignmentTarget, Expression};

    // Extract the property name and object from the member expression
    let (prop_name, span, object_expr) = match &assignment.left {
        AssignmentTarget::StaticMemberExpression(static_member) => (
            static_member.property.name.as_str(),
            static_member.property.span,
            &static_member.object,
        ),
        AssignmentTarget::ComputedMemberExpression(computed_member) => {
            // Handle this['ngOnInit'] or component['ngOnDestroy']
            let prop_name = match &computed_member.expression {
                Expression::StringLiteral(lit) => lit.value.as_str(),
                _ => return,
            };
            (prop_name, computed_member.expression.span(), &computed_member.object)
        }
        _ => return,
    };

    // Check if this is a lifecycle method name
    if !is_lifecycle_method(prop_name) {
        return;
    }

    // Exclude assignments to .prototype.* (e.g., type.prototype.ngOnDestroy = ...)
    // These are valid ways to add lifecycle methods
    if is_prototype_assignment(object_expr) {
        return;
    }

    ctx.diagnostic(require_lifecycle_on_prototype_diagnostic(span, prop_name));
}

fn is_prototype_assignment(expr: &oxc_ast::ast::Expression) -> bool {
    use oxc_ast::ast::Expression;

    // Check for patterns like: type.prototype, (type.prototype as any), type['prototype']
    match expr {
        Expression::StaticMemberExpression(static_member) => {
            static_member.property.name.as_str() == "prototype"
        }
        Expression::ComputedMemberExpression(computed_member) => {
            matches!(
                &computed_member.expression,
                Expression::StringLiteral(lit) if lit.value.as_str() == "prototype"
            )
        }
        Expression::TSAsExpression(as_expr) => is_prototype_assignment(&as_expr.expression),
        Expression::ParenthesizedExpression(paren) => is_prototype_assignment(&paren.expression),
        _ => false,
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

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Regular method
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnInit() {}
        }
        ",
        // Multiple lifecycle methods as regular methods
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnInit() {}
            ngOnDestroy() {}
            ngAfterViewInit() {}
        }
        ",
        // Arrow function property that's not a lifecycle method
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            handleClick = () => {};
        }
        ",
        // Non-Angular class with lifecycle property
        r"
        class TestClass {
            ngOnInit = () => {};
        }
        ",
        // Assigning to prototype is valid
        r"
        @Component({})
        class Test {}
        function hook(type) {
            type.prototype.ngOnDestroy = () => {};
        }
        hook(Test);
        ",
        // Assigning to prototype with type cast
        r"
        @Component({})
        class Test {}
        function hook(type) {
            (type.prototype as any).ngOnDestroy = () => {};
        }
        hook(Test);
        ",
        // Assigning to prototype with bracket notation
        r"
        @Component({})
        class Test {}
        function hook(type) {
            type['prototype'].ngOnDestroy = () => {};
        }
        hook(Test);
        ",
        // Property not named after lifecycle method
        r"
        @Component({})
        class Test {
            onDestroy = () => {}
        }
        ",
        // Assignment to non-lifecycle property in constructor
        r"
        @Component({})
        class Test {
            constructor() {
                this.onDestroy = () => {}
            }
        }
        ",
        // Local variable assignment (not a member)
        r"
        @Component({})
        class Test {
            constructor() {
                let ngOnDestroy;
                ngOnDestroy = () => {};
            }
        }
        ",
    ];

    let fail = vec![
        // Arrow function lifecycle
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnInit = () => {};
        }
        ",
        // Function expression lifecycle
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnDestroy = function() {};
        }
        ",
        // Multiple arrow function lifecycles
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnInit = () => {};
            ngOnChanges = () => {};
        }
        ",
        // Directive with arrow lifecycle
        r"
        import { Directive } from '@angular/core';
        @Directive({
            selector: '[appTest]'
        })
        class TestDirective {
            ngAfterViewInit = () => {};
        }
        ",
        // Injectable with arrow lifecycle
        r"
        import { Injectable } from '@angular/core';
        @Injectable({ providedIn: 'root' })
        class TestService {
            ngOnDestroy = () => {};
        }
        ",
        // Property initialized to non-function value
        r"
        import { Component } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            ngOnInit = func;
        }
        ",
        // Assignment in constructor
        r"
        class Test {
            constructor() {
                this.ngOnDestroy = func;
            }
        }
        ",
        // Assignment in constructor with bracket notation
        r"
        class Test {
            constructor() {
                this['ngOnDestroy'] = func;
            }
        }
        ",
        // Assignment in method
        r"
        class Test {
            run() {
                this.ngOnDestroy = func;
            }
        }
        ",
        // Assignment outside class
        r"
        function hook(component) {
            component.ngOnDestroy = func;
        }
        ",
        // Assignment with type cast
        r"
        function hook(component) {
            (component as any).ngOnDestroy = func;
        }
        ",
        // Computed property name with string literal
        r"
        class Test {
            ['ngOnDestroy'] = func;
        }
        ",
    ];

    Tester::new(RequireLifecycleOnPrototype::NAME, RequireLifecycleOnPrototype::PLUGIN, pass, fail)
        .test_and_snapshot();
}
