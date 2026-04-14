use oxc_ast::{AstKind, ast::{Expression, ObjectPropertyKind, PropertyKey}};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use lazy_regex::Regex;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::get_decorator_name,
};

fn use_injectable_provided_in_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `providedIn` property is mandatory for `Injectables`")
        .with_help(
            "Add `providedIn: 'root'` (or 'any', 'platform') to the @Injectable decorator.",
        )
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct UseInjectableProvidedInConfig {
    /// Regex pattern to ignore certain class names (e.g., "/Effects$/")
    /// This matches ESLint's `ignoreClassNamePattern` option
    #[serde(default)]
    ignore_class_name_pattern: Option<String>
}

#[derive(Debug, Clone, Default)]
pub struct UseInjectableProvidedIn {
    /// Compiled regex pattern from ignoreClassNamePattern option
    ignore_pattern: Option<Regex>
}

impl From<UseInjectableProvidedInConfig> for UseInjectableProvidedIn {
    fn from(config: UseInjectableProvidedInConfig) -> Self {
        let ignore_pattern = config.ignore_class_name_pattern.and_then(|pattern| {
            // ESLint uses patterns like "/Effects$/" - strip the slashes if present
            let pattern = pattern.trim();
            let pattern = if pattern.starts_with('/') && pattern.ends_with('/') {
                &pattern[1..pattern.len()-1]
            } else {
                pattern
            };
            Regex::new(pattern).ok()
        });
        Self { ignore_pattern }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `@Injectable` classes use the `providedIn` property for tree-shaking.
    ///
    /// ### Why is this bad?
    ///
    /// Without `providedIn`, services must be added to a module's `providers` array,
    /// which prevents tree-shaking. This means:
    /// - Unused services are included in the bundle
    /// - Larger bundle sizes
    /// - Reduced application performance
    ///
    /// Using `providedIn: 'root'` (or another value) enables tree-shaking, allowing
    /// the compiler to remove unused services from the final bundle.
    ///
    /// Note: Classes implementing `HttpInterceptor` are excluded from this rule as
    /// they require a different registration pattern.
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/use-injectable-provided-in": ["error", {
    ///     "ignoreClassNamePattern": ".*Interceptor$"
    ///   }]
    /// }
    /// ```
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Injectable } from '@angular/core';
    ///
    /// @Injectable()
    /// export class MyService {}
    ///
    /// @Injectable({})
    /// export class AnotherService {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Injectable } from '@angular/core';
    ///
    /// @Injectable({ providedIn: 'root' })
    /// export class MyService {}
    ///
    /// @Injectable({ providedIn: 'any' })
    /// export class ScopedService {}
    /// ```
    UseInjectableProvidedIn,
    angular,
    pedantic,
    pending
);

impl Rule for UseInjectableProvidedIn {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        serde_json::from_value::<UseInjectableProvidedInConfig>(config_value.clone())
            .map(Into::into)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Injectable decorator (exact match only)
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Injectable" {
            return;
        }

        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Find the parent class
        let Some(class) = get_parent_class_from_decorator(node, ctx) else {
            return;
        };

        // Check if class name matches ignore pattern or implements HttpInterceptor
        if let Some(class_name) = class.id.as_ref().map(|id| id.name.as_str()) {
            // Check ignore pattern (regex)
            if let Some(ref pattern) = self.ignore_pattern {
                if pattern.is_match(class_name) {
                    return;
                }
            }

            // Skip HttpInterceptor implementations (check class name or implements clause)
            if implements_http_interceptor(class) {
                return;
            }
        }

        // Get the decorator's call expression to check arguments
        let call_expr = match &decorator.expression {
            Expression::CallExpression(call) => call,
            _ => return, // Not a call expression, skip
        };

        // If no arguments, report error on decorator
        if call_expr.arguments.is_empty() {
            ctx.diagnostic(use_injectable_provided_in_diagnostic(decorator.span));
            return;
        }

        // Get the first argument
        let first_arg = &call_expr.arguments[0];

        // If the argument is not an object expression (e.g., a variable reference),
        // we cannot statically analyze it - skip (matches ESLint behavior)
        let Some(metadata) = first_arg.as_expression().and_then(|expr| {
            match expr.without_parentheses() {
                Expression::ObjectExpression(obj) => Some(obj.as_ref()),
                _ => None, // Variable reference like @Injectable(options) - skip
            }
        }) else {
            return;
        };

        // Check if providedIn is present with a valid value
        // Use our custom implementation that handles computed property keys
        match get_provided_in_property(metadata) {
            ProvidedInResult::NotPresent => {
                // No providedIn property - report error on decorator
                ctx.diagnostic(use_injectable_provided_in_diagnostic(decorator.span));
            }
            ProvidedInResult::NullValue(span) => {
                // providedIn: null - report error on the null value
                ctx.diagnostic(use_injectable_provided_in_diagnostic(span));
            }
            ProvidedInResult::UndefinedValue(span) => {
                // providedIn: undefined - report error on the undefined value
                ctx.diagnostic(use_injectable_provided_in_diagnostic(span));
            }
            ProvidedInResult::ValidValue => {
                // Has a valid providedIn value - no error
            }
        }
    }
}

/// Result of checking for providedIn property
enum ProvidedInResult {
    /// No providedIn property found
    NotPresent,
    /// providedIn is set to null (with span of the null literal)
    NullValue(Span),
    /// providedIn is set to undefined (with span of the undefined identifier)
    UndefinedValue(Span),
    /// providedIn has a valid value
    ValidValue,
}

/// Get the providedIn property from an object expression, handling various key formats:
/// - `providedIn: value` (identifier key)
/// - `'providedIn': value` (string literal key)
/// - `['providedIn']: value` (computed string literal key)
/// - `` [`providedIn`]: value `` (computed template literal key)
fn get_provided_in_property(obj: &oxc_ast::ast::ObjectExpression<'_>) -> ProvidedInResult {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(property) = prop else {
            continue;
        };

        // Check if this property key is 'providedIn'
        let is_provided_in = match &property.key {
            // Non-computed identifier: providedIn
            PropertyKey::StaticIdentifier(ident) if !property.computed => {
                ident.name.as_str() == "providedIn"
            }
            // Non-computed string literal: 'providedIn' or "providedIn"
            PropertyKey::StringLiteral(lit) if !property.computed => {
                lit.value.as_str() == "providedIn"
            }
            // Computed string literal: ['providedIn'] or ["providedIn"]
            PropertyKey::StringLiteral(lit) if property.computed => {
                lit.value.as_str() == "providedIn"
            }
            // Computed template literal: [`providedIn`]
            PropertyKey::TemplateLiteral(template) if property.computed => {
                // Only handle simple template literals without expressions
                template.expressions.is_empty()
                    && template.quasis.len() == 1
                    && template.quasis[0].value.raw.as_str() == "providedIn"
            }
            // Computed identifier: [providedIn] where providedIn is a variable
            // We cannot statically determine the key value, so skip it
            _ => false,
        };

        if is_provided_in {
            // Check the value
            return match &property.value {
                Expression::NullLiteral(null_lit) => {
                    ProvidedInResult::NullValue(null_lit.span)
                }
                Expression::Identifier(ident) if ident.name.as_str() == "undefined" => {
                    ProvidedInResult::UndefinedValue(ident.span)
                }
                _ => ProvidedInResult::ValidValue,
            };
        }
    }
    ProvidedInResult::NotPresent
}

fn get_parent_class_from_decorator<'a, 'b>(
    node: &'b crate::AstNode<'a>,
    ctx: &'b LintContext<'a>,
) -> Option<&'b oxc_ast::ast::Class<'a>> {
    for ancestor in ctx.nodes().ancestors(node.id()) {
        if let AstKind::Class(class) = ancestor.kind() {
            return Some(class);
        }
    }
    None
}

fn implements_http_interceptor(class: &oxc_ast::ast::Class<'_>) -> bool {
    if class.implements.is_empty() {
        return false;
    }
    class.implements.iter().any(|ts_impl| {
        match &ts_impl.expression {
            // Simple identifier: HttpInterceptor
            oxc_ast::ast::TSTypeName::IdentifierReference(ident) => {
                ident.name.as_str() == "HttpInterceptor"
                    || ident.name.as_str() == "HttpInterceptorFn"
            }
            // Qualified name: ng.HttpInterceptor (member expression)
            oxc_ast::ast::TSTypeName::QualifiedName(qualified) => {
                qualified.right.name.as_str() == "HttpInterceptor"
                    || qualified.right.name.as_str() == "HttpInterceptorFn"
            }
            // ThisExpression - not relevant for HttpInterceptor check
            oxc_ast::ast::TSTypeName::ThisExpression(_) => false,
        }
    })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // No decorator at all
        (
            r"class Test {}",
            None,
        ),
        // Variable reference as argument - cannot statically analyze
        (
            r"
    const options = {};
    @Injectable(options)
    class Test {}
  ",
            None,
        ),
        // providedIn with template literal value
        (
            r"
    @Injectable({
      providedIn: `any`,
    })
    class Test {}
  ",
            None,
        ),
        // providedIn with string literal key (quoted)
        (
            r"
    @Injectable({
      'providedIn': 'root',
    })
    class Test {}
  ",
            None,
        ),
        // providedIn with computed string literal key
        (
            r"
    @Injectable({
      ['providedIn']: SomeModule,
    })
    class Test {}
  ",
            None,
        ),
        // providedIn with computed template literal key
        (
            r"
    @Injectable({
      [`providedIn`]: providedIn(),
    })
    class Test {}
  ",
            None,
        ),
        // HttpInterceptor implementation
        (
            r"
    @Injectable()
    class Test implements HttpInterceptor {}
  ",
            None,
        ),
        // Qualified HttpInterceptor (ng.HttpInterceptor)
        (
            r"
    @Injectable()
    class Test implements ng.HttpInterceptor {}
  ",
            None,
        ),
        // Custom ignore pattern - Effects suffix
        (
            r"
        @Injectable()
        class TestEffects {}
      ",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/Effects$/" }])),
        ),
        // Custom decorator (not @Injectable)
        (
            r"
    @CustomInjectable()
    class Test {}
  ",
            None,
        ),
    ];

    let fail = vec![
        // No arguments - error on decorator
        (
            r"
      @Injectable()

      class Test {}
    ",
            None,
        ),
        // Empty object - error on decorator
        (
            r"
      @Injectable({})

      class Test {}
    ",
            None,
        ),
        // Computed property key with variable - cannot determine key statically, no providedIn found
        (
            r"
      const providedIn = 'anotherProperty';
      @Injectable({ [providedIn]: [] })

      class Test {}
    ",
            None,
        ),
        // providedIn: null - error on the null value
        (
            r"
      @Injectable({ providedIn: null })

      class Test {}
    ",
            None,
        ),
        // providedIn: undefined (computed string literal key) - error on undefined value
        (
            r"
      @Injectable({ ['providedIn']: undefined })

      class Test {}

      @Injectable()
      class HttpPostInterceptor implements HttpInterceptor {}
    ",
            None,
        ),
        // providedIn: undefined with ignore pattern (computed string literal key)
        (
            r"
      @Injectable({ ['providedIn']: undefined })

      class Test {}

      @Injectable()
      class ProvidedInNgModule {}
    ",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        ),
        // providedIn: undefined (computed template literal key)
        (
            r"
      @Injectable({ [`providedIn`]: undefined })

      class Test {}

      @Injectable()
      class TestEffects {}
    ",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        ),
    ];

    Tester::new(UseInjectableProvidedIn::NAME, UseInjectableProvidedIn::PLUGIN, pass, fail)
        .test_and_snapshot();
}
