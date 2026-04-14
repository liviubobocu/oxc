use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_name,
        get_metadata_property,
    },
};

fn pipe_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    let prefix_list = prefixes.join(", ");
    OxcDiagnostic::warn(format!("Pipe names should be prefixed with: {prefix_list}"))
        .with_help(
            "Use a consistent prefix for pipe names to avoid naming collisions and to make it \
            clear which pipes belong to your application.",
        )
        .with_label(span)
}

fn selector_after_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    let prefix_list = prefixes.join(", ");
    OxcDiagnostic::warn(format!(
        "Pipes should have a selector after the {prefix_list} prefix"
    ))
    .with_help("A pipe name cannot be just the prefix - add a descriptive name after the prefix.")
    .with_label(span)
}

#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PipePrefixConfig {
    #[serde(default)]
    prefixes: Vec<String>
}

#[derive(Debug, Clone, Default)]
pub struct PipePrefix {
    prefixes: Vec<String>
}

impl From<PipePrefixConfig> for PipePrefix {
    fn from(config: PipePrefixConfig) -> Self {
        Self { prefixes: config.prefixes }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent prefix for pipe names.
    ///
    /// ### Why is this bad?
    ///
    /// Using a prefix for pipe names helps:
    /// - Avoid naming collisions with Angular built-in pipes or third-party pipes
    /// - Easily identify which pipes belong to your application
    /// - Maintain consistency across the codebase
    ///
    /// ### Configuration
    ///
    /// This rule requires configuration to specify the allowed prefixes:
    ///
    /// ```json
    /// {
    ///   "angular/pipe-prefix": ["error", { "prefixes": ["app", "my"] }]
    /// }
    /// ```
    ///
    /// ### Examples
    ///
    /// With configuration `{ "prefixes": ["app"] }`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Pipe, PipeTransform } from '@angular/core';
    ///
    /// @Pipe({
    ///   name: 'formatDate'  // Missing prefix
    /// })
    /// export class FormatDatePipe implements PipeTransform {
    ///   transform(value: Date): string {
    ///     return value.toISOString();
    ///   }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Pipe, PipeTransform } from '@angular/core';
    ///
    /// @Pipe({
    ///   name: 'appFormatDate'  // Has 'app' prefix
    /// })
    /// export class FormatDatePipe implements PipeTransform {
    ///   transform(value: Date): string {
    ///     return value.toISOString();
    ///   }
    /// }
    /// ```
    PipePrefix,
    angular,
    pedantic,
    pending,
    config = PipePrefixConfig
);

impl Rule for PipePrefix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        serde_json::from_value::<PipePrefixConfig>(config_value.clone()).map(Into::into)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // If no prefixes configured, skip the rule
        if self.prefixes.is_empty() {
            return;
        }

        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Pipe decorator
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Pipe" {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Get the name property value expression (for accurate span reporting)
        let Some(name_expr) = get_metadata_property(metadata, "name") else {
            return;
        };

        // Extract the string value from the name expression
        let pipe_name = match name_expr {
            oxc_ast::ast::Expression::StringLiteral(lit) => lit.value.as_str(),
            oxc_ast::ast::Expression::TemplateLiteral(lit) => {
                if lit.expressions.is_empty() && lit.quasis.len() == 1 {
                    lit.quasis[0].value.raw.as_str()
                } else {
                    return;
                }
            }
            _ => return,
        };

        // Get the span for error reporting (use name value span, matching ESLint)
        let name_span = name_expr.span();

        // ESLint's pipe-prefix has two-step validation:
        // 1. prefixValidator - checks if name starts with prefix followed by uppercase or nothing
        // 2. selectorAfterPrefixValidator - checks if there's something after the prefix
        //
        // We need to check both to match ESLint's behavior

        // First, check if the pipe name matches any prefix with proper casing
        // (prefix followed by uppercase or nothing)
        let prefix_check_result = self.prefixes.iter().find_map(|prefix| {
            if pipe_name.starts_with(prefix) {
                let rest = &pipe_name[prefix.len()..];
                // Check if rest is empty OR starts with uppercase (camelCase convention)
                if rest.is_empty() || rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    Some((true, rest.is_empty()))
                } else {
                    None
                }
            } else {
                None
            }
        });

        match prefix_check_result {
            Some((_, rest_is_empty)) => {
                // Prefix matches with proper casing, but check if there's something after
                if rest_is_empty {
                    // Report: pipe name is just the prefix with nothing after
                    ctx.diagnostic(selector_after_prefix_diagnostic(name_span, &self.prefixes));
                }
                // If rest_is_empty is false, it's valid - do nothing
            }
            None => {
                // No valid prefix found - report prefix error
                ctx.diagnostic(pipe_prefix_diagnostic(name_span, &self.prefixes));
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Correct prefix
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'appFormatDate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Multiple allowed prefixes
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'myFormatDate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app", "my"] }])),
        ),
        // No prefixes configured (rule disabled)
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'formatDate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            None,
        ),
        // Non-Angular Pipe
        (
            r"
            import { Pipe } from 'other-lib';
            @Pipe({
                name: 'formatDate'
            })
            class FormatDatePipe {}
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Template literal pipe name with correct prefix
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: `appFormatDate`
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
    ];

    let fail = vec![
        // Missing prefix
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'formatDate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Wrong prefix
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'myFormatDate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Prefix without proper casing
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'appformatdate'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Template literal missing prefix
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: `formatDate`
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
        // Pipe name is just the prefix with nothing after (selectorAfterPrefixFailure)
        (
            r"
            import { Pipe, PipeTransform } from '@angular/core';
            @Pipe({
                name: 'app'
            })
            class FormatDatePipe implements PipeTransform {
                transform(value: Date): string {
                    return value.toISOString();
                }
            }
            ",
            Some(serde_json::json!([{ "prefixes": ["app"] }])),
        ),
    ];

    Tester::new(PipePrefix::NAME, PipePrefix::PLUGIN, pass, fail).test_and_snapshot();
}
