use oxc_ast::AstKind;
use oxc_ast::ast::{Expression, PropertyDefinition, TSType, TSTypeName};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use serde::Deserialize;

use crate::{AstNode, context::LintContext, rule::Rule, utils::get_decorator_name};

/// Known signal types that should be marked as readonly.
/// Matches ESLint's KNOWN_SIGNAL_TYPES from utils/signals.ts
const KNOWN_SIGNAL_TYPES: [&str; 5] = [
    "Signal",
    "InputSignal",
    "ModelSignal",
    "WritableSignal",
    "InputSignalWithTransform",
];

/// Known signal creation functions.
const KNOWN_SIGNAL_CREATION_FUNCTIONS: [&str; 10] = [
    "computed",
    "contentChild",
    "contentChildren",
    "input",
    "linkedSignal",
    "model",
    "signal",
    "toSignal",
    "viewChild",
    "viewChildren",
];

/// Query decorators that should be replaced with signal functions.
const QUERY_DECORATORS: [(&str, &str); 4] = [
    ("ViewChild", "viewChild"),
    ("ViewChildren", "viewChildren"),
    ("ContentChild", "contentChild"),
    ("ContentChildren", "contentChildren"),
];

fn prefer_readonly_signal_properties_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Properties declared using signals should be marked as `readonly` since they should not be reassigned",
    )
    .with_label(span)
}

fn prefer_input_signals_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Use `InputSignal`s (e.g. via `input()`) for Component input properties rather than the legacy `@Input()` decorator",
    )
    .with_label(span)
}

fn prefer_query_signals_diagnostic(
    span: Span,
    function_name: &str,
    decorator_name: &str,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Use the `{function_name}` function instead of the `{decorator_name}` decorator"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferSignalsConfig {
    /// Whether to check that signal properties are marked as readonly. Default: true.
    #[serde(default = "default_true")]
    pub prefer_readonly_signal_properties: bool,

    /// Whether to prefer input() over @Input(). Default: true.
    #[serde(default = "default_true")]
    pub prefer_input_signals: bool,

    /// Whether to prefer viewChild/contentChild over @ViewChild/@ContentChild. Default: true.
    #[serde(default = "default_true")]
    pub prefer_query_signals: bool,

    /// Whether to use type checking to infer signal types. Default: false.
    /// Note: This option is currently not fully supported in oxlint as it requires
    /// TypeScript's type checker. Currently only syntactic analysis is performed.
    #[serde(default)]
    pub use_type_checking: bool,

    /// Additional function names that create signals. Default: [].
    #[serde(default)]
    pub additional_signal_creation_functions: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct PreferSignals(Box<PreferSignalsConfig>);

impl Default for PreferSignals {
    fn default() -> Self {
        Self(Box::new(PreferSignalsConfig {
            prefer_readonly_signal_properties: true,
            prefer_input_signals: true,
            prefer_query_signals: true,
            use_type_checking: false,
            additional_signal_creation_functions: Vec::new(),
        }))
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the use of Angular signal-based APIs (`input()`, `output()`, `viewChild()`, etc.)
    /// over legacy decorators (`@Input()`, `@Output()`, `@ViewChild()`, etc.) and ensures that
    /// signal properties are marked as `readonly`.
    ///
    /// ### Why is this bad?
    ///
    /// Legacy decorators rely on Angular's zone-based change detection, which checks the entire
    /// component tree for changes. Signal-based APIs provide fine-grained reactivity where only
    /// affected parts of the UI are updated, resulting in better performance.
    ///
    /// Signal properties should be marked as `readonly` because signals themselves are stable
    /// references - you read their value by calling them, you don't reassign the signal.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Component, Input, ViewChild, signal } from '@angular/core';
    ///
    /// @Component({ selector: 'app-example', template: '' })
    /// export class ExampleComponent {
    ///   @Input() name: string;              // Should use input()
    ///   @ViewChild('container') container;  // Should use viewChild()
    ///   testSignal = signal(42);            // Should be readonly
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Component, input, viewChild, signal } from '@angular/core';
    ///
    /// @Component({ selector: 'app-example', template: '' })
    /// export class ExampleComponent {
    ///   readonly name = input<string>();
    ///   readonly container = viewChild<ElementRef>('container');
    ///   readonly testSignal = signal(42);
    /// }
    /// ```
    PreferSignals,
    angular,
    pedantic
);

impl Rule for PreferSignals {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let config = value
            .get(0)
            .map(|v| serde_json::from_value::<PreferSignalsConfig>(v.clone()))
            .transpose()?
            .unwrap_or_default();
        Ok(Self(Box::new(config)))
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            // Check for non-readonly signal properties
            AstKind::PropertyDefinition(prop_def) => {
                if self.0.prefer_readonly_signal_properties {
                    self.check_readonly_signal_property(prop_def, ctx);
                }
            }
            // Check for legacy decorators
            AstKind::Decorator(decorator) => {
                let Some(decorator_name) = get_decorator_name(decorator) else {
                    return;
                };

                // Check @Input() decorator
                if self.0.prefer_input_signals && decorator_name == "Input" {
                    ctx.diagnostic(prefer_input_signals_diagnostic(decorator.span));
                    return;
                }

                // Check query decorators (@ViewChild, @ViewChildren, @ContentChild, @ContentChildren)
                if self.0.prefer_query_signals {
                    if let Some((_, function_name)) =
                        QUERY_DECORATORS.iter().find(|(dec, _)| *dec == decorator_name)
                    {
                        ctx.diagnostic(prefer_query_signals_diagnostic(
                            decorator.span,
                            function_name,
                            decorator_name,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

impl PreferSignals {
    /// Check if a property definition should be marked as readonly because it's a signal.
    fn check_readonly_signal_property(&self, prop_def: &PropertyDefinition<'_>, ctx: &LintContext<'_>) {
        // Skip if already readonly
        if prop_def.readonly {
            return;
        }

        let mut should_be_readonly = false;

        // Check 1: Type annotation indicates a signal type
        if let Some(type_annotation) = &prop_def.type_annotation {
            if let TSType::TSTypeReference(type_ref) = &type_annotation.type_annotation {
                // Check if it has type arguments (Signal<T>, InputSignal<T>, etc.)
                if type_ref.type_arguments.is_some() {
                    if let TSTypeName::IdentifierReference(ident) = &type_ref.type_name {
                        if KNOWN_SIGNAL_TYPES.contains(&ident.name.as_str()) {
                            should_be_readonly = true;
                        }
                    }
                }
            }
        } else if let Some(value) = &prop_def.value {
            // Check 2: Value is a call to a signal creation function
            should_be_readonly = self.is_signal_creation_call(value);
        }

        if should_be_readonly {
            // Report on the property key (name), not the entire property definition
            let key_span = prop_def.key.span();
            ctx.diagnostic(prefer_readonly_signal_properties_diagnostic(key_span));
        }
    }

    /// Check if an expression is a call to a signal creation function.
    fn is_signal_creation_call(&self, expr: &Expression<'_>) -> bool {
        let Expression::CallExpression(call_expr) = expr else {
            return false;
        };

        // Handle .asReadonly() calls: signal(42).asReadonly()
        // We need to check the object being called
        if let Expression::StaticMemberExpression(member_expr) = &call_expr.callee {
            if member_expr.property.name.as_str() == "asReadonly" {
                // Check if the object is a signal creation call
                if let Expression::CallExpression(inner_call) = &member_expr.object {
                    return self.is_signal_creation_callee(&inner_call.callee);
                }
            }
        }

        self.is_signal_creation_callee(&call_expr.callee)
    }

    /// Check if a callee expression is a signal creation function or method.
    fn is_signal_creation_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            // Direct call: signal(), computed(), input(), etc.
            Expression::Identifier(ident) => {
                let name = ident.name.as_str();
                KNOWN_SIGNAL_CREATION_FUNCTIONS.contains(&name)
                    || self.0.additional_signal_creation_functions.iter().any(|f| f == name)
            }
            // Method call: input.required(), model.required(), viewChild.required(), etc.
            Expression::StaticMemberExpression(member_expr) => {
                // Only handle .required() method
                if member_expr.property.name.as_str() != "required" {
                    return false;
                }
                // Check if the object is a known signal creation function
                if let Expression::Identifier(ident) = &member_expr.object {
                    let name = ident.name.as_str();
                    KNOWN_SIGNAL_CREATION_FUNCTIONS.contains(&name)
                        || self.0.additional_signal_creation_functions.iter().any(|f| f == name)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Non-signal class properties
        r"
        class Test {
            testSubject = new Subject();
        }
        ",
        r"
        class Test {
            testSubject = new ReplaySubject(1);
        }
        ",
        r"
        class Test {
            testValue = test();
        }
        ",
        r"
        class Test {
            testValue: number;
        }
        ",
        r"
        class Test {
            testValue: Widget<number>;
        }
        ",
        // Readonly signal type annotations (correct)
        r"
        class Test {
            readonly testSignal: Signal<number>;
        }
        ",
        r"
        class Test {
            readonly testSignal: InputSignal<number>;
        }
        ",
        r"
        class Test {
            readonly testSignal: ModelSignal<number>;
        }
        ",
        r"
        class Test {
            readonly testSignal: WritableSignal<number>;
        }
        ",
        // Readonly signal creation functions (correct)
        r"
        class Test {
            readonly testSignal = computed(() => 0);
        }
        ",
        r"
        class Test {
            readonly testSignal = linkedSignal(() => source);
        }
        ",
        r"
        class Test {
            readonly testSignal = contentChild('test');
        }
        ",
        r"
        class Test {
            readonly testSignal = contentChild.required('test');
        }
        ",
        r"
        class Test {
            readonly testSignal = contentChildren('test');
        }
        ",
        r"
        class Test {
            readonly testSignal = input('');
        }
        ",
        r"
        class Test {
            readonly testSignal = input.required();
        }
        ",
        r"
        class Test {
            readonly testSignal = model();
            readonly testRequired = model.required(42);
        }
        ",
        r"
        class Test {
            readonly testSignal = signal(true);
        }
        ",
        r"
        class Test {
            readonly testSignal = signal(true).asReadonly();
        }
        ",
        r"
        class Test {
            readonly testSignal = toSignal(source);
        }
        ",
        r"
        class Test {
            readonly testSignal = viewChild('test');
        }
        ",
        r"
        class Test {
            readonly testSignal = viewChild.required('test');
        }
        ",
        r"
        class Test {
            readonly testSignal = viewChildren('test');
        }
        ",
        // Unknown function - doesn't require readonly
        r"
        class Test {
            testSignal = createSignal('test');
        }
        ",
        // readonly input (correct for preferInputSignals)
        r"
        class Test {
            readonly value = input();
        }
        ",
        // readonly viewChild (correct for preferQuerySignals)
        r"
        class Test {
            readonly query = viewChild('test');
        }
        ",
        r"
        class Test {
            readonly query = viewChildren('test');
        }
        ",
        r"
        class Test {
            readonly query = contentChild('test');
        }
        ",
        r"
        class Test {
            readonly query = contentChildren('test');
        }
        ",
    ];

    let fail = vec![
        // Non-readonly Signal type (should be readonly)
        r"
        class Test {
            testSignal: Signal<number>;
        }
        ",
        // Non-readonly InputSignal type
        r"
        class Test {
            testSignal: InputSignal<number>;
        }
        ",
        // Non-readonly ModelSignal type
        r"
        class Test {
            testSignal: ModelSignal<number>;
        }
        ",
        // Non-readonly WritableSignal type
        r"
        class Test {
            testSignal: WritableSignal<number>;
        }
        ",
        // Non-readonly computed() call
        r"
        class Test {
            testSignal = computed(() => 0);
        }
        ",
        // Non-readonly linkedSignal() call
        r"
        class Test {
            testSignal = linkedSignal(() => source);
        }
        ",
        // Non-readonly contentChild() call
        r"
        class Test {
            testSignal = contentChild('test');
        }
        ",
        // Non-readonly contentChild.required() call
        r"
        class Test {
            testSignal = contentChild.required('test');
        }
        ",
        // Non-readonly contentChildren() call
        r"
        class Test {
            testSignal = contentChildren('test');
        }
        ",
        // Non-readonly input() call
        r"
        class Test {
            testSignal = input('');
        }
        ",
        // Non-readonly input.required() call
        r"
        class Test {
            testSignal = input.required('');
        }
        ",
        // Non-readonly model() call
        r"
        class Test {
            testSignal = model(42);
        }
        ",
        // Non-readonly model.required() call
        r"
        class Test {
            testSignal = model.required();
        }
        ",
        // Non-readonly signal() call
        r"
        class Test {
            testSignal = signal(42);
        }
        ",
        // Non-readonly signal().asReadonly() call
        r"
        class Test {
            testSignal = signal(42).asReadonly();
        }
        ",
        // Non-readonly toSignal() call
        r"
        class Test {
            testSignal = toSignal(source);
        }
        ",
        // Non-readonly viewChild() call
        r"
        class Test {
            testSignal = viewChild('test');
        }
        ",
        // Non-readonly viewChild.required() call
        r"
        class Test {
            testSignal = viewChild.required('test');
        }
        ",
        // Non-readonly viewChildren() call
        r"
        class Test {
            testSignal = viewChildren('test');
        }
        ",
        // Legacy @Input() decorator
        r"
        class Test {
            @Input()
            value = 1;
        }
        ",
        // Legacy @ViewChild() decorator
        r"
        class Test {
            @ViewChild('test')
            value: Widget;
        }
        ",
        // Legacy @ViewChildren() decorator
        r"
        class Test {
            @ViewChildren('test')
            value: QueryList<Widget>;
        }
        ",
        // Legacy @ContentChild() decorator
        r"
        class Test {
            @ContentChild('test')
            value: Widget;
        }
        ",
        // Legacy @ContentChildren() decorator
        r"
        class Test {
            @ContentChildren('test')
            value: QueryList<Widget>;
        }
        ",
    ];

    Tester::new(PreferSignals::NAME, PreferSignals::PLUGIN, pass, fail).test_and_snapshot();
}

#[test]
fn test_with_options() {
    use crate::tester::Tester;

    // Test with preferReadonlySignalProperties: false
    let pass_no_readonly = vec![(
        r"
        class Test {
            testSignal = signal('test');
        }
        ",
        Some(serde_json::json!([{ "preferReadonlySignalProperties": false }])),
    )];

    // Test with preferInputSignals: false
    let pass_no_input_signals = vec![(
        r"
        class Test {
            @Input()
            readonly value = 1;
        }
        ",
        Some(serde_json::json!([{ "preferInputSignals": false }])),
    )];

    // Test with preferQuerySignals: false
    let pass_no_query_signals = vec![
        (
            r"
            class Test {
                @ViewChild('test')
                value: Widget;
            }
            ",
            Some(serde_json::json!([{ "preferQuerySignals": false }])),
        ),
        (
            r"
            class Test {
                @ViewChildren('test')
                value: QueryList<Widget>;
            }
            ",
            Some(serde_json::json!([{ "preferQuerySignals": false }])),
        ),
        (
            r"
            class Test {
                @ContentChild('test')
                value: Widget;
            }
            ",
            Some(serde_json::json!([{ "preferQuerySignals": false }])),
        ),
        (
            r"
            class Test {
                @ContentChildren('test')
                value: QueryList<Widget>;
            }
            ",
            Some(serde_json::json!([{ "preferQuerySignals": false }])),
        ),
    ];

    // Test with additionalSignalCreationFunctions
    let fail_additional_functions = vec![(
        r"
        class Test {
            testSignal = createSignal('test');
        }
        ",
        Some(serde_json::json!([{ "additionalSignalCreationFunctions": ["createSignal"] }])),
    )];

    Tester::new(PreferSignals::NAME, PreferSignals::PLUGIN, pass_no_readonly, vec![])
        .test_and_snapshot();

    Tester::new(PreferSignals::NAME, PreferSignals::PLUGIN, pass_no_input_signals, vec![])
        .test_and_snapshot();

    Tester::new(PreferSignals::NAME, PreferSignals::PLUGIN, pass_no_query_signals, vec![])
        .test_and_snapshot();

    Tester::new(PreferSignals::NAME, PreferSignals::PLUGIN, vec![], fail_additional_functions)
        .test_and_snapshot();
}
