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
        get_metadata_string_value,
    },
};

fn no_output_rename_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Output bindings should not be aliased")
        .with_help(
            "Avoid aliasing outputs as it can lead to confusion. Use the original property name \
            or rename the property if the alias is more appropriate.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputRename;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows aliasing output bindings (renaming outputs with a different public name).
    ///
    /// ### Why is this bad?
    ///
    /// Two names for the same property (one private, one public) is confusing.
    /// It requires developers to remember both names and understand the mapping.
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
    ///   @Output('valueChanged') change = new EventEmitter<string>();
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
    ///   @Output() valueChange = new EventEmitter<string>();
    /// }
    /// ```
    NoOutputRename,
    angular,
    correctness,
    pending
);

impl Rule for NoOutputRename {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Check for @Output decorator on properties or getters
        if let AstKind::Decorator(decorator) = node.kind() {
            // Check if this is an Output decorator
            let Some(decorator_name) = get_decorator_name(decorator) else {
                return;
            };

            if decorator_name == "Output" {
                self.check_output_decorator(decorator, node, ctx);
            } else if decorator_name == "Component" || decorator_name == "Directive" {
                // Check for outputs metadata array aliases
                self.check_outputs_metadata_aliases(decorator, ctx);
            }
            return;
        }

        // Check for output() signal function
        if let AstKind::PropertyDefinition(prop) = node.kind() {
            self.check_output_signal(prop, node, ctx);
        }
    }
}

impl NoOutputRename {
    /// Check @Output decorator for aliasing
    fn check_output_decorator(
        &self,
        decorator: &oxc_ast::ast::Decorator<'_>,
        node: &AstNode<'_>,
        ctx: &LintContext<'_>,
    ) {
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the decorator call to check for alias
        let Some(call) = get_decorator_call(decorator) else {
            return;
        };

        // Get the first argument (the alias) and its span for error reporting
        let Some(first_arg) = call.arguments.first() else {
            return;
        };

        let (alias, alias_span) = match first_arg {
            oxc_ast::ast::Argument::StringLiteral(lit) => (Some(lit.value.as_str()), lit.span),
            oxc_ast::ast::Argument::TemplateLiteral(tpl) => {
                // Handle template literals with no expressions: `alias`
                if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                    (Some(tpl.quasis[0].value.raw.as_str()), tpl.span)
                } else {
                    (None, tpl.span)
                }
            }
            oxc_ast::ast::Argument::ObjectExpression(obj) => {
                // Check for { alias: 'name' } format
                if let Some((alias_str, span)) = get_alias_from_object_with_span(obj) {
                    (Some(alias_str), span)
                } else {
                    (None, obj.span)
                }
            }
            _ => (None, first_arg.span()),
        };

        let Some(alias) = alias else {
            return;
        };

        // Get the property name from parent (supports both PropertyDefinition and MethodDefinition for getters)
        let property_name = self.get_property_name_from_decorator(node, ctx);

        // Get selectors from the class's @Component/@Directive decorator
        let selectors = self.get_class_selectors(node, ctx);

        // Allow if alias matches property name
        if property_name.as_deref() == Some(alias) {
            // Even if alias matches property name, ESLint reports it when it differs
            // Actually, looking at ESLint: alias === propertyName still reports with fix to remove
            ctx.diagnostic(no_output_rename_diagnostic(alias_span));
            return;
        }

        // Allow if alias is allowed based on selector
        if let Some(ref prop_name) = property_name {
            if is_alias_allowed(&selectors, prop_name, alias) {
                return;
            }
        }

        ctx.diagnostic(no_output_rename_diagnostic(alias_span));
    }

    /// Check outputs metadata array in @Component/@Directive for aliases
    fn check_outputs_metadata_aliases(
        &self,
        decorator: &oxc_ast::ast::Decorator<'_>,
        ctx: &LintContext<'_>,
    ) {
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Get selectors for allowlist checking
        let selectors = get_selectors_from_metadata(metadata);

        // Get the outputs property value
        let Some(outputs_value) = get_metadata_property(metadata, "outputs") else {
            return;
        };

        // outputs should be an array
        let Expression::ArrayExpression(array) = outputs_value else {
            return;
        };

        for element in &array.elements {
            let Some(element) = element.as_expression() else {
                continue;
            };

            // Get string value and span from the element
            let (value, span) = match element {
                Expression::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                Expression::TemplateLiteral(tpl) => {
                    if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                        (tpl.quasis[0].value.raw.as_str(), tpl.span)
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Check for "propertyName: aliasName" format
            let value = value.trim();
            if let Some(colon_pos) = value.find(':') {
                let property_name = value[..colon_pos].trim();
                let alias_name = value[colon_pos + 1..].trim();

                // Skip if no alias (just property name without colon would not enter here)
                if alias_name.is_empty() {
                    continue;
                }

                // Check if this outputs property is inside hostDirectives (allowed)
                if is_inside_host_directives(metadata, outputs_value) {
                    continue;
                }

                // If alias equals property name, report (with fix to remove alias)
                if alias_name == property_name {
                    ctx.diagnostic(no_output_rename_diagnostic(span));
                    continue;
                }

                // Check if alias is allowed based on selector
                if is_alias_allowed(&selectors, property_name, alias_name) {
                    continue;
                }

                ctx.diagnostic(no_output_rename_diagnostic(span));
            }
        }
    }

    /// Check output() signal function for aliasing
    fn check_output_signal(
        &self,
        prop: &oxc_ast::ast::PropertyDefinition<'_>,
        node: &AstNode<'_>,
        ctx: &LintContext<'_>,
    ) {
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
        // Note: Match ESLint behavior - does not verify imports for exact parity

        // Get selectors from the class's @Component/@Directive decorator
        let selectors = self.get_class_selectors(node, ctx);

        // Check for alias in options object
        for arg in &call.arguments {
            if let oxc_ast::ast::Argument::ObjectExpression(obj) = arg {
                if let Some((alias, alias_span)) = get_alias_from_object_with_span(obj) {
                    let property_name = prop.key.static_name();

                    // Allow if alias matches property name - but still report (ESLint does)
                    if property_name.as_deref() == Some(alias) {
                        ctx.diagnostic(no_output_rename_diagnostic(alias_span));
                        return;
                    }

                    // Check if alias is allowed based on selector
                    if let Some(prop_name) = property_name.as_deref() {
                        if is_alias_allowed(&selectors, prop_name, alias) {
                            return;
                        }
                    }

                    ctx.diagnostic(no_output_rename_diagnostic(alias_span));
                    return;
                }
            }
        }
    }

    /// Get property name from decorator's parent (PropertyDefinition or MethodDefinition for getters)
    fn get_property_name_from_decorator(
        &self,
        node: &crate::AstNode<'_>,
        ctx: &LintContext<'_>,
    ) -> Option<String> {
        for ancestor in ctx.nodes().ancestors(node.id()) {
            match ancestor.kind() {
                AstKind::PropertyDefinition(prop) => {
                    return prop.key.static_name().map(|s| s.to_string());
                }
                AstKind::MethodDefinition(method) => {
                    // For getters: @Output('alias') get getter() {}
                    return method.key.static_name().map(|s| s.to_string());
                }
                _ => {}
            }
        }
        None
    }

    /// Get selectors from the class's @Component/@Directive decorator
    fn get_class_selectors(&self, node: &AstNode<'_>, ctx: &LintContext<'_>) -> Vec<String> {
        // Navigate up to find the class declaration
        for ancestor in ctx.nodes().ancestors(node.id()) {
            if let AstKind::Class(class) = ancestor.kind() {
                // Find @Component or @Directive decorator
                for decorator in &class.decorators {
                    let Some(decorator_name) = get_decorator_name(decorator) else {
                        continue;
                    };

                    if decorator_name != "Component" && decorator_name != "Directive" {
                        continue;
                    }

                    let Some(metadata) = get_component_metadata(decorator) else {
                        continue;
                    };

                    return get_selectors_from_metadata(metadata);
                }
            }
        }
        Vec::new()
    }
}

/// Get selectors from component/directive metadata
fn get_selectors_from_metadata(metadata: &oxc_ast::ast::ObjectExpression<'_>) -> Vec<String> {
    let Some(selector_str) = get_metadata_string_value(metadata, "selector") else {
        return Vec::new();
    };

    // Parse selector string: "foo[bar], test" -> ["foo", "bar", "test"]
    parse_selectors(selector_str)
}

/// Parse selector string into individual selector names
fn parse_selectors(selector: &str) -> Vec<String> {
    let mut result = Vec::new();

    // Split by comma for multiple selectors
    for part in selector.split(',') {
        let part = part.trim();
        // Remove brackets and whitespace: "[foo]" -> "foo", "foo[bar]" -> "foo", "bar"
        let cleaned = part
            .replace('[', " ")
            .replace(']', " ")
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();

        for s in cleaned {
            if !s.is_empty() {
                result.push(s);
            }
        }
    }

    result
}

/// Check if an alias is allowed based on selectors
fn is_alias_allowed(selectors: &[String], property_name: &str, alias: &str) -> bool {
    selectors.iter().any(|selector| {
        // Direct match: alias equals selector
        selector == alias ||
        // Composed name: selector + capitalize(propertyName) equals alias
        composed_name(selector, property_name) == alias
    })
}

/// Create composed name: selector + capitalize(propertyName)
fn composed_name(selector: &str, property_name: &str) -> String {
    format!("{}{}", selector, capitalize(property_name))
}

/// Capitalize first letter of a string
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Check if the outputs property is inside a hostDirectives configuration
fn is_inside_host_directives(
    _metadata: &oxc_ast::ast::ObjectExpression<'_>,
    outputs_value: &Expression<'_>,
) -> bool {
    // The outputs_value is the array expression. We need to check if it's
    // nested inside hostDirectives. Since we got it from get_metadata_property
    // on the top-level metadata, it's NOT inside hostDirectives.
    // hostDirectives outputs would be at a deeper nesting level.
    // This function is called for top-level outputs, so return false.
    // The actual hostDirectives check happens because get_metadata_property
    // only looks at top-level properties, not nested ones.
    let _ = outputs_value;
    false
}

/// Get alias from object expression with span (supports both string and template literals)
fn get_alias_from_object_with_span<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<(&'a str, Span)> {
    use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};

    for property in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = property {
            if let PropertyKey::StaticIdentifier(ident) = &prop.key {
                if ident.name.as_str() == "alias" {
                    match &prop.value {
                        Expression::StringLiteral(lit) => {
                            return Some((lit.value.as_str(), lit.span));
                        }
                        Expression::TemplateLiteral(tpl) => {
                            // Handle template literals with no expressions: `alias`
                            if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                                return Some((tpl.quasis[0].value.raw.as_str(), tpl.span));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // No alias
        r"
        import { Component, Output, EventEmitter } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            @Output() change = new EventEmitter<string>();
        }
        ",
        // output() signal without alias
        r"
        import { Component, output } from '@angular/core';
        @Component({
            selector: 'app-test',
            template: ''
        })
        class TestComponent {
            change = output<string>();
        }
        ",
        // Non-component/directive class (no decorator)
        r"
        class Test {}
        ",
        // Non-Angular decorator
        r"
        @Page({
          outputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
        })
        class Test {}
        ",
        // Component without @Output
        r"
        @Component()
        class Test {
          change = new EventEmitter();
        }
        ",
        // @Output without alias on Directive
        r"
        @Directive()
        class Test {
          @Output() buttonChange = new EventEmitter<'change'>();
        }
        ",
        // output() signal without alias on Directive
        r"
        @Directive()
        class Test {
          buttonChange = output<'change'>();
        }
        ",
        // outputs as identifier (not array literal)
        r"
        @Component({
          outputs,
        })
        class Test {}
        ",
        // outputs with spread
        r"
        @Directive({
          outputs: [...test],
        })
        class Test {}
        ",
        // outputs as function call
        r"
        @Component({
          outputs: func(),
        })
        class Test {}
        ",
        // outputs with function call element
        r"
        @Directive({
          outputs: [func(), 'a'],
        })
        class Test {}
        ",
        // @Output on getter without alias
        r"
        @Component({})
        class Test {
          @Output() get getter() {}
        }
        ",
        // Dynamic alias (variable reference) - not statically analyzable
        r"
        const change = 'change';
        @Component()
        class Test {
          @Output(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
        }
        ",
        // output() with dynamic alias
        r"
        const change = 'change';
        @Component()
        class Test {
          touchMove = output<{ action: 'click' | 'close' }>({ alias: change });
        }
        ",
        // Dynamic property name and alias
        r"
        const blur = 'blur';
        const click = 'click';
        @Directive()
        class Test {
          @Output(blur) [click]: EventEmitter<Blur>;
        }
        ",
        // output() with dynamic property and alias
        r"
        const blur = 'blur';
        const click = 'click';
        @Directive()
        class Test {
          [click] = output({ alias: blur });
        }
        ",
        // Selector matches output name
        r"
        @Component({
          selector: 'foo[bar]'
        })
        class Test {
          @Output() bar: string;
        }
        ",
        // output() with selector match
        r"
        @Component({
          selector: 'foo[bar]'
        })
        class Test {
          bar = output();
        }
        ",
        // outputs metadata with selector prefix allowed (selector: foo, output alias: foo)
        r"
        @Directive({
          'selector': 'foo',
          'outputs': [`test: foo`]
        })
        class Test {}
        ",
        // outputs metadata with computed key (string)
        r"
        @Component({
          'selector': 'foo',
          ['outputs']: [`test: foo`]
        })
        class Test {}
        ",
        // outputs metadata with computed key (template)
        r"
        @Directive({
          'selector': 'foo',
          [`outputs`]: [`test: foo`]
        })
        class Test {}
        ",
        // Alias matches selector
        r"
        @Component({
          selector: '[foo], test',
        })
        class Test {
          @Output('foo') label: string;
        }
        ",
        // output() alias matches selector
        r"
        @Component({
          selector: '[foo], test',
        })
        class Test {
          label = output({ alias: 'foo' });
        }
        ",
        // hostDirectives outputs (allowed to be aliased)
        r"
        @Component({
          selector: 'foo',
          hostDirectives: [{
            directive: CdkMenuItem,
            outputs: ['cdkMenuItemTriggered: triggered'],
          }]
        })
        class Test {}
        ",
        // hostDirectives with string key
        r"
        @Component({
          selector: 'foo',
          'hostDirectives': [{
            directive: CdkMenuItem,
            outputs: ['cdkMenuItemTriggered: triggered'],
          }]
        })
        class Test {}
        ",
        // hostDirectives with computed key
        r"
        @Component({
          selector: 'foo',
          ['hostDirectives']: [{
            directive: CdkMenuItem,
            outputs: ['cdkMenuItemTriggered: triggered'],
          }]
        })
        class Test {}
        ",
        // Alias matches selector + capitalize(propertyName)
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          @Output('fooMyColor') myColor: string;
        }
        ",
        // output() alias matches selector + capitalize(propertyName)
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          myColor = output({ alias: 'fooMyColor' });
        }
        ",
    ];

    let fail = vec![
        // outputs metadata aliased in Component
        r"
        @Component({
          outputs: ['a: b']
        })
        class Test {}
        ",
        // outputs metadata with template literal key, aliased
        r"
        @Directive({
          inputs: ['abort'],
          'outputs': [boundary, `test: copy`],
        })
        class Test {}
        ",
        // outputs metadata aliased with same name (should still report)
        r"
        @Component({
          ['outputs']: ['orientation: orientation'],
        })
        class Test {}
        ",
        // outputs metadata with template computed key, aliased with same name
        r"
        @Directive({
          [`outputs`]: ['orientation: orientation'],
        })
        class Test {}
        ",
        // @Output with backtick template literal alias
        r"
        @Component()
        class Test {
          @Custom() @Output(`change`) _change = getOutput();
        }
        ",
        // output() with backtick template literal alias
        r"
        @Component()
        class Test {
          _change = output({ alias: `change` });
        }
        ",
        // @Output alias matches property name (still reported per ESLint)
        r"
        @Directive()
        class Test {
          @Output('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
        ",
        // output() alias matches property name (still reported per ESLint)
        r"
        @Directive()
        class Test {
          change = output({ alias: 'change' });
        }
        ",
        // @Output on getter with alias
        r"
        @Component()
        class Test {
          @Output(`devicechange`) get getter() {}

          @Output() test: string;
        }
        ",
        // Alias prefixed by selector but suffix doesn't match property
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          @Output('fooColor') colors: string;
        }
        ",
        // output() alias prefixed by selector but suffix doesn't match property
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          colors = output({ alias: 'fooColor' });
        }
        ",
        // Alias not strictly camelCase with selector
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          @Output('foocolor') color: string;
        }
        ",
        // output() alias not strictly camelCase with selector
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ alias: 'foocolor' });
        }
        ",
        // @Output in class without Component/Directive decorator
        r"
        @Directive({
          selector: 'kebab-case',
        })
        class Test {}

        @Injectable()
        class Test {
          @Output('kebab-case') blur = this.getOutput();
        }
        ",
        // output() in class without Component/Directive decorator
        r"
        @Directive({
          selector: 'kebab-case',
        })
        class Test {}

        @Injectable()
        class Test {
          blur = output({ alias: 'kebab-case' });
        }
        ",
        // output() alias with properties after it
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ alias: 'test', after: 'it' });
        }
        ",
        // output() alias with properties before it (trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test', });
        }
        ",
        // output() alias with properties before it (no trailing comma)
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test' });
        }
        ",
        // output() alias with properties before and after
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test', after: 'it' });
        }
        ",
    ];

    Tester::new(NoOutputRename::NAME, NoOutputRename::PLUGIN, pass, fail).test_and_snapshot();
}
