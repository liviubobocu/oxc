use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_call, get_decorator_name, get_metadata_property,
        is_native_event_name,
    },
};

fn no_output_native_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Output bindings, including aliases, should not be named as standard DOM events")
        .with_help(
            "Using native event names like 'click', 'change', 'blur' can cause confusion and \
            unexpected behavior. Rename your output to something more descriptive like 'itemClick' \
            or 'valueChange'.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputNative;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows naming outputs with the same names as native DOM events.
    ///
    /// ### Why is this bad?
    ///
    /// Naming an output the same as a native DOM event (e.g., `click`, `change`, `blur`)
    /// can lead to confusion about whether the event is a native DOM event or a custom
    /// Angular output. It can also cause unexpected behavior when the event bubbles or
    /// is captured at a higher level in the DOM.
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
    ///   @Output() click = new EventEmitter<void>(); // Same as native DOM event
    ///   @Output() change = new EventEmitter<string>(); // Same as native DOM event
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
    ///   @Output() itemClick = new EventEmitter<void>();
    ///   @Output() valueChange = new EventEmitter<string>();
    /// }
    /// ```
    NoOutputNative,
    angular,
    correctness,
    pending
);

impl Rule for NoOutputNative {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            // Check @Component/@Directive decorators for outputs metadata array
            AstKind::Decorator(decorator) => {
                self.check_outputs_metadata(decorator, ctx);
            }
            // Check property definitions for @Output decorator or output() signal
            AstKind::PropertyDefinition(prop) => {
                self.check_output_property(prop, ctx);
            }
            // Check method definitions (getters) with @Output decorator
            AstKind::MethodDefinition(method) => {
                self.check_output_getter(method, ctx);
            }
            _ => {}
        }
    }
}

impl NoOutputNative {
    /// Check outputs metadata array in @Component/@Directive decorators
    fn check_outputs_metadata(&self, decorator: &oxc_ast::ast::Decorator<'_>, ctx: &LintContext<'_>) {
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        // Only check @Component and @Directive decorators
        if decorator_name != "Component" && decorator_name != "Directive" {
            return;
        }

        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        let Some(outputs_value) = get_metadata_property(metadata, "outputs") else {
            return;
        };

        let Expression::ArrayExpression(array) = outputs_value else {
            return;
        };

        // Check each element in the outputs array
        for element in &array.elements {
            let Some(expr) = element.as_expression() else {
                continue;
            };

            // Get the string value and span from the element
            let (value, span) = match expr {
                Expression::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
                    if let Some(quasi) = tpl.quasis.first() {
                        (quasi.value.raw.as_str(), quasi.span)
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Parse "propertyName: aliasName" format
            let (property_name, alias_name) = parse_output_string(value);

            // Check if property name is a native event
            if is_native_event_name(property_name) {
                ctx.diagnostic(no_output_native_diagnostic(span));
            }
            // Check if alias name is a native event (only if different from property name)
            else if let Some(alias) = alias_name {
                if is_native_event_name(alias) {
                    ctx.diagnostic(no_output_native_diagnostic(span));
                }
            }
        }
    }

    /// Check property definitions for @Output decorator or output() signal
    fn check_output_property(
        &self,
        prop: &oxc_ast::ast::PropertyDefinition<'_>,
        ctx: &LintContext<'_>,
    ) {
        // Check for @Output decorator
        for decorator in &prop.decorators {
            let Some(decorator_name) = get_decorator_name(decorator) else {
                continue;
            };

            if decorator_name != "Output" {
                continue;
            }

            // Check for alias in decorator arguments
            if let Some(call) = get_decorator_call(decorator) {
                if let Some(first_arg) = call.arguments.first() {
                    match first_arg {
                        // @Output('alias')
                        oxc_ast::ast::Argument::StringLiteral(lit) => {
                            if is_native_event_name(lit.value.as_str()) {
                                ctx.diagnostic(no_output_native_diagnostic(lit.span));
                            }
                        }
                        // @Output(`alias`)
                        oxc_ast::ast::Argument::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
                            if let Some(quasi) = tpl.quasis.first() {
                                if is_native_event_name(quasi.value.raw.as_str()) {
                                    ctx.diagnostic(no_output_native_diagnostic(quasi.span));
                                }
                            }
                        }
                        // @Output({ alias: 'name' })
                        oxc_ast::ast::Argument::ObjectExpression(obj) => {
                            if let Some((alias, alias_span)) = get_alias_from_object_with_span(obj) {
                                if is_native_event_name(alias) {
                                    ctx.diagnostic(no_output_native_diagnostic(alias_span));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Also check property name (ESLint checks BOTH alias AND property name)
            if let Some(name) = prop.key.static_name() {
                if is_native_event_name(&name) {
                    let span = get_property_key_span(&prop.key).unwrap_or(prop.key.span());
                    ctx.diagnostic(no_output_native_diagnostic(span));
                }
            }
            return;
        }

        // Check for output() signal function
        let Some(value) = &prop.value else {
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

        // Check for alias in options object
        for arg in &call.arguments {
            if let oxc_ast::ast::Argument::ObjectExpression(obj) = arg {
                if let Some((alias, alias_span)) = get_alias_from_object_with_span(obj) {
                    if is_native_event_name(alias) {
                        ctx.diagnostic(no_output_native_diagnostic(alias_span));
                    }
                }
            }
        }

        // Also check property name for output() signals
        if let Some(name) = prop.key.static_name() {
            if is_native_event_name(&name) {
                let span = get_property_key_span(&prop.key).unwrap_or(prop.key.span());
                ctx.diagnostic(no_output_native_diagnostic(span));
            }
        }
    }

    /// Check method definitions (getters) with @Output decorator
    fn check_output_getter(
        &self,
        method: &oxc_ast::ast::MethodDefinition<'_>,
        ctx: &LintContext<'_>,
    ) {
        // Only check getters
        if method.kind != oxc_ast::ast::MethodDefinitionKind::Get {
            return;
        }

        // Check for @Output decorator
        for decorator in &method.decorators {
            let Some(decorator_name) = get_decorator_name(decorator) else {
                continue;
            };

            if decorator_name != "Output" {
                continue;
            }

            // Check for alias in decorator arguments
            if let Some(call) = get_decorator_call(decorator) {
                if let Some(first_arg) = call.arguments.first() {
                    match first_arg {
                        // @Output('alias')
                        oxc_ast::ast::Argument::StringLiteral(lit) => {
                            if is_native_event_name(lit.value.as_str()) {
                                ctx.diagnostic(no_output_native_diagnostic(lit.span));
                            }
                        }
                        // @Output(`alias`)
                        oxc_ast::ast::Argument::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
                            if let Some(quasi) = tpl.quasis.first() {
                                if is_native_event_name(quasi.value.raw.as_str()) {
                                    ctx.diagnostic(no_output_native_diagnostic(quasi.span));
                                }
                            }
                        }
                        // @Output({ alias: 'name' })
                        oxc_ast::ast::Argument::ObjectExpression(obj) => {
                            if let Some((alias, alias_span)) = get_alias_from_object_with_span(obj) {
                                if is_native_event_name(alias) {
                                    ctx.diagnostic(no_output_native_diagnostic(alias_span));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Also check getter name (ESLint checks BOTH alias AND getter name)
            if let Some(name) = method.key.static_name() {
                if is_native_event_name(&name) {
                    let span = get_method_key_span(&method.key).unwrap_or(method.key.span());
                    ctx.diagnostic(no_output_native_diagnostic(span));
                }
            }
            return;
        }
    }
}

/// Parse an output string in "propertyName: aliasName" format
fn parse_output_string(value: &str) -> (&str, Option<&str>) {
    if let Some(colon_pos) = value.find(':') {
        let property_name = value[..colon_pos].trim();
        let alias_name = value[colon_pos + 1..].trim();
        (property_name, Some(alias_name))
    } else {
        (value.trim(), None)
    }
}

fn get_alias_from_object_with_span<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<(&'a str, Span)> {
    use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};

    for property in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = property
            && let PropertyKey::StaticIdentifier(ident) = &prop.key
            && ident.name.as_str() == "alias"
        {
            match &prop.value {
                Expression::StringLiteral(lit) => {
                    return Some((lit.value.as_str(), lit.span));
                }
                Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
                    if let Some(quasi) = tpl.quasis.first() {
                        return Some((quasi.value.raw.as_str(), quasi.span));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn get_property_key_span(key: &oxc_ast::ast::PropertyKey<'_>) -> Option<Span> {
    use oxc_ast::ast::PropertyKey;

    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.span),
        PropertyKey::PrivateIdentifier(ident) => Some(ident.span),
        PropertyKey::StringLiteral(lit) => Some(lit.span),
        PropertyKey::TemplateLiteral(tpl) => tpl.quasis.first().map(|q| q.span),
        _ => None,
    }
}

fn get_method_key_span(key: &oxc_ast::ast::PropertyKey<'_>) -> Option<Span> {
    get_property_key_span(key)
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Empty class
        r"class Test {}",
        // Non-Angular decorator with native event names in outputs
        r#"
        @Page({
          outputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
        })
        class Test {}
        "#,
        // Property without @Output decorator
        r"
        @Component()
        class Test {
          change = new EventEmitter();
        }
        ",
        // Custom output name with @Output
        r"
        @Directive()
        class Test {
          @Output() buttonChange = new EventEmitter<'change'>();
        }
        ",
        // output() signal with custom name
        r"
        @Directive()
        class Test {
          buttonChange = output<'change'>();
        }
        ",
        // Case-sensitive check (Drag != drag)
        r"
        @Component()
        class Test {
          @Output() Drag = new EventEmitter<{ click: string }>();
        }
        ",
        // output() signal with uppercase name
        r"
        @Component()
        class Test {
          Drag = output<{ click: string }>();
        }
        ",
        // Alias that is not a native event
        r"
        @Directive()
        class Test {
          @Output(`changelower`) changeText = new EventEmitter<{ bar: string, blur: string }>();
        }
        ",
        // output() signal with non-native alias
        r"
        @Directive()
        class Test {
          changeText = output<{ bar: string, blur: string }>({ alias: `changelower` });
        }
        ",
        // Aliased with custom name
        r"
        @Component()
        class Test {
          @Output('buttonChange') changelower = new EventEmitter<ComplextObject>();
        }
        ",
        // output() signal aliased with custom name
        r"
        @Component()
        class Test {
          changelower = output<ComplextObject>({ alias: 'buttonChange' });
        }
        ",
        // SVgZoom is not a native event (case sensitive)
        r"
        @Directive()
        class Test<SVGScroll> {
          @Output() SVgZoom = new EventEmitter<SVGScroll>();
        }
        ",
        // output() signal with non-native name
        r"
        @Directive()
        class Test<SVGScroll> {
           SVgZoom = output<SVGScroll>();
        }
        ",
        // Variable alias (not a string literal)
        r"
        const change = 'change';
        @Component()
        class Test {
          @Output(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
        }
        ",
        // output() signal with variable alias
        r"
        const change = 'change';
        @Component()
        class Test {
          touchMove = output<{ action: 'click' | 'close' }>({ alias: change });
        }
        ",
        // Computed property names (not analyzed)
        r"
        const blur = 'blur';
        const click = 'click';
        @Directive()
        class Test {
          @Output(blur) [click]: EventEmitter<Blur>;
        }
        ",
        // output() signal with computed property
        r"
        const blur = 'blur';
        const click = 'click';
        @Directive()
        class Test {
          [click] = output<Blur>({ alias: blur });
        }
        ",
        // outputs metadata with custom name
        r#"
        @Component({
          selector: 'foo',
          'outputs': [`test: foo`]
        })
        class Test {}
        "#,
        // outputs metadata with computed key (string literal)
        r#"
        @Directive({
          selector: 'foo',
          ['outputs']: [`test: foo`]
        })
        class Test {}
        "#,
        // outputs metadata with computed key (template literal)
        r#"
        @Component({
          'selector': 'foo',
          [`outputs`]: [`test: foo`]
        })
        class Test {}
        "#,
        // getter with custom name
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
        // outputs metadata with native event name
        r#"
        @Component({
          'outputs': ['pagehide']
        })
        class Test {}
        "#,
        // outputs metadata with aliased native event name
        r#"
        @Directive({
          inputs: ['abort'],
          ['outputs']: [boundary, `test: copy`],
        })
        class Test {}
        "#,
        // outputs metadata with computed template literal key
        r#"
        @Component({
          inputs: ['abort'],
          [`outputs`]: [boundary, `test: copy`],
        })
        class Test {}
        "#,
        // outputs metadata with native property name (before colon)
        r#"
        @Directive({
          outputs: ['orientationchange: orientation'],
        })
        class Test {}
        "#,
        // @Output property with native event name
        r"
        @Component()
        class Test {
          @Output() change: EventEmitter<any> = new EventEmitter<{}>();
        }
        ",
        // output() signal with native event name
        r"
        @Component()
        class Test {
          change = output();
        }
        ",
        // @Output property with quoted native event name
        r"
        @Directive()
        class Test {
          @Output() @Custom('change') 'change' = new EventEmitter<void>();
        }
        ",
        // output() signal with quoted native event name
        r"
        @Directive()
        class Test {
          'change' = output();
        }
        ",
        // @Output with template literal alias to native event
        r"
        @Component()
        class Test {
          @Custom() @Output(`change`) _change = getOutput();
        }
        ",
        // output() signal with template literal alias to native event
        r"
        @Component()
        class Test {
          _change = output({ alias: `change` });
        }
        ",
        // @Output with string alias to native event
        r"
        @Directive()
        class Test {
          @Output('change') _change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
        ",
        // output() signal with string alias to native event
        r"
        @Directive()
        class Test {
          _change = output({ alias: 'change' });
        }
        ",
        // getter with native event name in quotes
        r"
        @Component()
        class Test {
          @Output('getter') get 'cut'() {}
        }
        ",
        // getter with template literal alias to native event
        r"
        @Directive()
        class Test {
          @Output(`devicemotion`) get getter() {}
        }
        ",
        // Both alias AND property name are native events (reports both)
        r"
        @Injectable()
        class Test {
          @Output('click') blur = this.getOutput();
        }
        ",
        // output() signal with both alias AND property as native events
        r"
        @Injectable()
        class Test {
          blur = output({ alias: 'click' });
        }
        ",
    ];

    Tester::new(NoOutputNative::NAME, NoOutputNative::PLUGIN, pass, fail).test_and_snapshot();
}
