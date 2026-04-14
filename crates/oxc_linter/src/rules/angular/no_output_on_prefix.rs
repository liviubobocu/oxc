use oxc_ast::AstKind;
use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{get_component_metadata, get_decorator_call, get_decorator_name, has_on_prefix},
};

fn no_output_on_prefix_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Output bindings, including aliases, should not be named \"on\" or prefixed with it: `{name}`"
    ))
    .with_help(
        "Remove the 'on' prefix from the output name. In Angular templates, outputs are already \
        bound with (output) syntax which implies an event. Adding 'on' is redundant: \
        (onClick)=\"handler()\" becomes confusing.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputOnPrefix;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows naming outputs with the "on" prefix.
    ///
    /// ### Why is this bad?
    ///
    /// In Angular templates, outputs are bound using the `(output)` syntax which already
    /// implies an event handler. Adding an "on" prefix creates redundancy:
    ///
    /// ```html
    /// <!-- Confusing: (onClick) suggests handling a click twice -->
    /// <app-button (onClick)="handleClick()"></app-button>
    ///
    /// <!-- Clear: (click) or (buttonClick) is more intuitive -->
    /// <app-button (click)="handleClick()"></app-button>
    /// ```
    ///
    /// The "on" prefix is a convention from DOM event handlers (`onclick`, `onblur`) but
    /// is not appropriate for Angular outputs.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component, Output, EventEmitter } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Output() onClick = new EventEmitter<void>();
    ///   @Output() onValueChange = new EventEmitter<string>();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, Output, EventEmitter } from '@angular/core';
    ///
    /// @Component({
    ///   selector: 'app-example',
    ///   template: ''
    /// })
    /// export class ExampleComponent {
    ///   @Output() click = new EventEmitter<void>();
    ///   @Output() valueChange = new EventEmitter<string>();
    /// }
    /// ```
    NoOutputOnPrefix,
    angular,
    correctness,
    pending
);

impl Rule for NoOutputOnPrefix {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::Decorator(decorator) => {
                let Some(decorator_name) = get_decorator_name(decorator) else {
                    return;
                };

                match decorator_name {
                    // Pattern 1: @Output() decorator on properties or getters
                    "Output" => {
                        self.check_output_decorator(node, decorator, ctx);
                    }
                    // Pattern 2: @Component or @Directive with outputs metadata array
                    "Component" | "Directive" => {
                        self.check_outputs_metadata(decorator, ctx);
                    }
                    _ => {}
                }
            }
            // Pattern 3: output() signal function on properties
            AstKind::PropertyDefinition(prop) => {
                self.check_output_signal(prop, ctx);
            }
            _ => {}
        }
    }
}

impl NoOutputOnPrefix {
    /// Check @Output() decorator on properties or getters for "on" prefix
    fn check_output_decorator<'a>(
        &self,
        node: &AstNode<'a>,
        decorator: &oxc_ast::ast::Decorator<'a>,
        ctx: &LintContext<'a>,
    ) {
        // ESLint matches @Output on any class, not just Component/Directive
        // So we skip the class check to match ESLint behavior

        // Get the property/getter name and its span
        let Some((output_name, property_span)) = get_decorated_member_name_with_span(node, ctx)
        else {
            return;
        };

        // Check for alias in decorator arguments (returns (alias_name, alias_span) if present)
        let alias_info = get_output_alias_with_span(decorator);

        // ESLint reports BOTH property name AND alias if both have "on" prefix
        // Check property name first
        if has_on_prefix(&output_name) {
            ctx.diagnostic(no_output_on_prefix_diagnostic(property_span, &output_name));
        }

        // Check alias if present (always check, even if property name was reported)
        if let Some((alias, alias_span)) = alias_info {
            if has_on_prefix(&alias) {
                ctx.diagnostic(no_output_on_prefix_diagnostic(alias_span, &alias));
            }
        }
    }

    /// Check outputs metadata array in @Component/@Directive for "on" prefix
    fn check_outputs_metadata<'a>(
        &self,
        decorator: &oxc_ast::ast::Decorator<'a>,
        ctx: &LintContext<'a>,
    ) {
        // Get the metadata object from the decorator
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Find the outputs property (supports identifier, string literal, and computed keys)
        let outputs_array = find_outputs_array(metadata);
        let Some(outputs_array) = outputs_array else {
            return;
        };

        // Check each element in the outputs array
        for element in &outputs_array.elements {
            // Only check string literals and template literals
            let (output_string, span) = match element {
                ArrayExpressionElement::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                ArrayExpressionElement::TemplateLiteral(tpl)
                    if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
                {
                    (tpl.quasis[0].value.raw.as_str(), tpl.span)
                }
                _ => continue, // Skip non-literal elements (identifiers, spread, function calls)
            };

            // Parse the output string: 'propertyName' or 'propertyName: aliasName'
            let trimmed = output_string.replace(char::is_whitespace, "");
            let parts: Vec<&str> = trimmed.split(':').collect();

            let property_name = parts.first().copied().unwrap_or("");
            let alias_name = parts.get(1).copied().unwrap_or("");

            // Check both property name and alias for "on" prefix
            if has_on_prefix(property_name)
                || (!alias_name.is_empty() && has_on_prefix(alias_name))
            {
                ctx.diagnostic(no_output_on_prefix_diagnostic(span, &trimmed));
            }
        }
    }

    /// Check output() signal function for "on" prefix
    fn check_output_signal<'a>(
        &self,
        prop: &oxc_ast::ast::PropertyDefinition<'a>,
        ctx: &LintContext<'a>,
    ) {
        let Some(value) = prop.value.as_ref() else {
            return;
        };

        let Expression::CallExpression(call) = value else {
            return;
        };

        let Expression::Identifier(callee) = &call.callee else {
            return;
        };

        if callee.name.as_str() != "output" {
            return;
        }

        // Get property name and span
        let Some((output_name, property_span)) = get_property_key_name_with_span(&prop.key) else {
            return;
        };

        // Check for alias in options object
        let alias_info = get_output_signal_alias_with_span(call);

        // Check property name
        if has_on_prefix(&output_name) {
            ctx.diagnostic(no_output_on_prefix_diagnostic(property_span, &output_name));
        }

        // Check alias if present
        if let Some((alias, alias_span)) = alias_info {
            if has_on_prefix(&alias) {
                ctx.diagnostic(no_output_on_prefix_diagnostic(alias_span, &alias));
            }
        }
    }
}

/// Find the outputs array in the metadata object.
/// Supports: `outputs: [...]`, `'outputs': [...]`, `['outputs']: [...]`, `[\`outputs\`]: [...]`
fn find_outputs_array<'a>(
    metadata: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<&'a oxc_ast::ast::ArrayExpression<'a>> {
    for prop in &metadata.properties {
        let ObjectPropertyKind::ObjectProperty(obj_prop) = prop else {
            continue;
        };

        let is_outputs_key = match &obj_prop.key {
            // Static identifier: outputs
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str() == "outputs",
            // String literal (non-computed): 'outputs'
            PropertyKey::StringLiteral(lit) => lit.value.as_str() == "outputs",
            // Computed template literal: [`outputs`]
            PropertyKey::TemplateLiteral(template) => {
                // Only match if it's a static template (no expressions)
                if template.expressions.is_empty() && template.quasis.len() == 1 {
                    template.quasis.first().is_some_and(|q| q.value.raw.as_str() == "outputs")
                } else {
                    false
                }
            }
            _ => {
                // Check for computed string literal: ['outputs']
                if obj_prop.computed {
                    match obj_prop.key.as_expression() {
                        Some(Expression::StringLiteral(lit)) => lit.value.as_str() == "outputs",
                        Some(Expression::TemplateLiteral(tpl))
                            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
                        {
                            tpl.quasis.first().is_some_and(|q| q.value.raw.as_str() == "outputs")
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            }
        };

        if is_outputs_key {
            // Return the array expression if the value is an array
            if let Expression::ArrayExpression(arr) = &obj_prop.value {
                return Some(arr.as_ref());
            }
        }
    }
    None
}

/// Get the property/getter name and span from the parent of a decorator node.
fn get_decorated_member_name_with_span<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Option<(String, Span)> {
    // The parent of the decorator should be the property/method definition
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

/// Get the alias from @Output() decorator arguments with its span.
/// Handles: @Output('alias'), @Output(`alias`), @Output({ alias: 'value' }), @Output({ alias: `value` })
fn get_output_alias_with_span(decorator: &oxc_ast::ast::Decorator<'_>) -> Option<(String, Span)> {
    let call_expr = get_decorator_call(decorator)?;

    // @Output('alias'), @Output(`alias`), or @Output({ alias: 'alias' })
    let first_arg = call_expr.arguments.first()?;

    match first_arg {
        oxc_ast::ast::Argument::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        oxc_ast::ast::Argument::TemplateLiteral(tpl)
            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 =>
        {
            Some((tpl.quasis[0].value.raw.to_string(), tpl.span))
        }
        oxc_ast::ast::Argument::ObjectExpression(obj) => get_alias_from_object_with_span(obj),
        _ => None,
    }
}

/// Get alias from output() signal function options.
fn get_output_signal_alias_with_span(
    call: &oxc_ast::ast::CallExpression<'_>,
) -> Option<(String, Span)> {
    for arg in &call.arguments {
        if let oxc_ast::ast::Argument::ObjectExpression(obj) = arg {
            if let Some(result) = get_alias_from_object_with_span(obj) {
                return Some(result);
            }
        }
    }
    None
}

/// Get alias value and span from an object expression.
/// Handles: { alias: 'value' } and { alias: `value` }
fn get_alias_from_object_with_span(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
) -> Option<(String, Span)> {
    for property in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = property
            && let PropertyKey::StaticIdentifier(ident) = &prop.key
            && ident.name.as_str() == "alias"
        {
            // Handle both string literal and template literal alias values
            match &prop.value {
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

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Plain class (no Angular decorator)
        r"class Test {}",
        // Non-Angular decorator (@Page instead of @Component)
        r"
        @Page({
            outputs: ['on', onChange, `onLine`, 'on: on2', 'offline: on', ...onCheck, onOutput()],
        })
        class Test {}
        ",
        // @Component without @Output decorator - property without prefix
        r"
        @Component()
        class Test {
            on = new EventEmitter();
        }
        ",
        // @Output property with valid name (not on prefix)
        r"
        @Directive()
        class Test {
            @Output() buttonChange = new EventEmitter<'on'>();
        }
        ",
        // output() signal with valid name (not on prefix)
        r"
        @Directive()
        class Test {
            buttonChange = output<'on'>();
        }
        ",
        // Uppercase prefix (case-sensitive - 'On' is not 'on')
        r"
        @Component()
        class Test {
            @Output() On = new EventEmitter<{ on: onType }>();
        }
        ",
        // output() signal with uppercase prefix
        r"
        @Component()
        class Test {
            On = output<{ on: onType }>();
        }
        ",
        // Template literal alias without forbidden prefix ('one' not 'on')
        r"
        @Directive()
        class Test {
            @Output(`one`) ontype = new EventEmitter<{ bar: string, on: boolean }>();
        }
        ",
        // output() signal with object alias without forbidden prefix
        r"
        @Directive()
        class Test {
            ontype = output<{ bar: string, on: boolean }>({ alias: `one` });
        }
        ",
        // String alias without forbidden prefix ('oneProp' starts with 'one' not 'on')
        r"
        @Component()
        class Test {
            @Output('oneProp') common = new EventEmitter<ComplextOn>();
        }
        ",
        // output() signal with object alias starting with 'one'
        r"
        @Component()
        class Test {
            common = output<ComplextOn>({ alias: 'oneProp' });
        }
        ",
        // ALL CAPS property name (case-sensitive)
        r"
        @Directive()
        class Test<On> {
            @Output() ON = new EventEmitter<On>();
        }
        ",
        // output() signal with ALL CAPS property name
        r"
        @Directive()
        class Test<On> {
            ON = output<On>();
        }
        ",
        // Variable reference as alias (not a string literal - can't analyze)
        r"
        const on = 'on';
        @Component()
        class Test {
            @Output(on) touchMove: EventEmitter<{ action: 'on' | 'off' }> = new EventEmitter<{ action: 'on' | 'off' }>();
        }
        ",
        // output() signal with variable reference as alias
        r"
        const on = 'on';
        @Component()
        class Test {
            touchMove = output<{ action: 'on' | 'off' }>({ alias: on });
        }
        ",
        // Computed property key (not a string literal - can't analyze)
        r"
        const test = 'on';
        const on = 'on';
        @Directive()
        class Test {
            @Output(test) [on]: EventEmitter<OnTest>;
        }
        ",
        // output() signal with computed property key
        r"
        const test = 'on';
        const on = 'on';
        @Directive()
        class Test {
            [on] = output<OnTest>({ alias: test });
        }
        ",
        // outputs metadata with valid names (string key)
        r"
        @Component({
            selector: 'foo',
            'outputs': [`test: foo`]
        })
        class Test {}
        ",
        // outputs metadata with computed string key
        r"
        @Directive({
            selector: 'foo',
            ['outputs']: [`test: foo`]
        })
        class Test {}
        ",
        // outputs metadata with computed template key
        r"
        @Component({
            'selector': 'foo',
            [`outputs`]: [`test: foo`]
        })
        class Test {}
        ",
        // Getter with valid name
        r"
        @Directive({
            selector: 'foo',
        })
        class Test {
            @Output() get 'getter'() {}
        }
        ",
    ];

    let fail = vec![
        // outputs metadata property named "on" in @Component
        r"
        @Component({
            outputs: ['on']
        })
        class Test {}
        ",
        // outputs metadata with aliased value "on" in @Directive
        r"
        @Directive({
            inputs: [onCredit],
            'outputs': [onLevel, `test: on`, onFunction()],
        })
        class Test {}
        ",
        // outputs metadata with computed string key and "onTest" property
        r"
        @Component({
            ['outputs']: ['onTest: test', ...onArray],
        })
        class Test {}
        ",
        // outputs metadata with computed template key and "onTest" property
        r"
        @Directive({
            [`outputs`]: ['onTest: test', ...onArray],
        })
        class Test {}
        ",
        // @Output property named "on" in @Component
        r"
        @Component()
        class Test {
            @Output() on: EventEmitter<any> = new EventEmitter<{}>();
        }
        ",
        // output() signal property named "on"
        r"
        @Component()
        class Test {
            on = output();
        }
        ",
        // @Output property with string literal key 'onPrefix'
        r"
        @Directive()
        class Test {
            @Output() @Custom('on') 'onPrefix' = new EventEmitter<void>();
        }
        ",
        // output() signal property with string literal key 'onPrefix'
        r"
        @Directive()
        class Test {
            'onPrefix' = output();
        }
        ",
        // @Output with template literal alias `on`
        r"
        @Component()
        class Test {
            @Custom() @Output(`on`) _on = getOutput();
        }
        ",
        // output() signal with template literal alias `on`
        r"
        @Component()
        class Test {
            _on = output({ alias: `on` });
        }
        ",
        // @Output with string alias 'onPrefix'
        r"
        @Directive()
        class Test {
            @Output('onPrefix') _on = (this.subject$ as Subject<{on: boolean}>).pipe();
        }
        ",
        // output() signal with string alias 'onPrefix'
        r"
        @Directive()
        class Test {
            _on = output({ alias: 'onPrefix' });
        }
        ",
        // @Output getter with string literal key 'on-getter'
        r"
        @Component()
        class Test {
            @Output('getter') get 'on-getter'() {}
        }
        ",
        // @Output getter with template literal alias `onGetter`
        r"
        @Directive()
        class Test {
            @Output(`onGetter`) get getter() {}
        }
        ",
        // @Output property named with prefix "on" AND aliased as "on" (two errors)
        r"
        @Injectable()
        class Test {
            @Output('on') onPrefix = this.getOutput();
        }
        ",
        // output() signal property named with prefix "on" AND aliased as "on" (two errors)
        r"
        @Injectable()
        class Test {
            onPrefix = output({ alias: 'on' });
        }
        ",
    ];

    Tester::new(NoOutputOnPrefix::NAME, NoOutputOnPrefix::PLUGIN, pass, fail).test_and_snapshot();
}
