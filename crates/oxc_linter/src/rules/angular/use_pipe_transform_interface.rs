use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        class_implements_interface, get_decorator_name
}
};

fn use_pipe_transform_interface_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Pipes should implement `PipeTransform` interface")
        .with_help(
            "Add `implements PipeTransform` to the class declaration and import \
            `PipeTransform` from '@angular/core'. This provides type safety for the \
            `transform` method.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UsePipeTransformInterface;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that classes decorated with `@Pipe` implement the `PipeTransform` interface.
    ///
    /// ### Why is this bad?
    ///
    /// Implementing the `PipeTransform` interface:
    /// - Provides type safety for the `transform` method signature
    /// - Improves IDE support with better autocompletion
    /// - Makes the code more self-documenting
    /// - Helps catch errors at compile time rather than runtime
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```typescript
    /// import { Pipe } from '@angular/core';
    ///
    /// @Pipe({
    ///   name: 'myPipe'
    /// })
    /// export class MyPipe {
    ///   transform(value: any): any {
    ///     return value;
    ///   }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// import { Pipe, PipeTransform } from '@angular/core';
    ///
    /// @Pipe({
    ///   name: 'myPipe'
    /// })
    /// export class MyPipe implements PipeTransform {
    ///   transform(value: any): any {
    ///     return value;
    ///   }
    /// }
    /// ```
    UsePipeTransformInterface,
    angular,
    correctness,
    pending
);

impl Rule for UsePipeTransformInterface {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };

        // Check if the class has a @Pipe decorator
        let pipe_decorator = class.decorators.iter().find(|decorator| {
            get_decorator_name(decorator).is_some_and(|name| name == "Pipe")
        });

        let Some(pipe_decorator) = pipe_decorator else {
            return;
        };

        // Note: Match ESLint behavior - trigger on ANY decorator named "Pipe"
        // ESLint does not verify imports, so we don't either for exact parity

        // Check if the class implements PipeTransform
        if class_implements_interface(class, "PipeTransform") {
            return;
        }

        // Report on the class identifier if present, otherwise on the decorator
        let span = class.id.as_ref().map_or(pipe_decorator.span, |id| id.span);
        ctx.diagnostic(use_pipe_transform_interface_diagnostic(span));
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // Implements PipeTransform
        r"
        import { Pipe, PipeTransform } from '@angular/core';
        @Pipe({
            name: 'myPipe'
        })
        class MyPipe implements PipeTransform {
            transform(value: any): any {
                return value;
            }
        }
        ",
        // Implements multiple interfaces including PipeTransform
        r"
        import { Pipe, PipeTransform, OnDestroy } from '@angular/core';
        @Pipe({
            name: 'myPipe'
        })
        class MyPipe implements PipeTransform, OnDestroy {
            transform(value: any): any {
                return value;
            }
            ngOnDestroy() {}
        }
        ",
        // Standalone pipe with interface
        r"
        import { Pipe, PipeTransform } from '@angular/core';
        @Pipe({
            name: 'myPipe',
            standalone: true
        })
        class MyPipe implements PipeTransform {
            transform(value: any): any {
                return value;
            }
        }
        ",
        // Non-Angular Pipe
        r"
        import { Pipe } from 'other-lib';
        @Pipe({
            name: 'myPipe'
        })
        class MyPipe {
            transform(value: any): any {
                return value;
            }
        }
        ",
    ];

    let fail = vec![
        // Missing PipeTransform interface
        r"
        import { Pipe } from '@angular/core';
        @Pipe({
            name: 'myPipe'
        })
        class MyPipe {
            transform(value: any): any {
                return value;
            }
        }
        ",
        // Standalone pipe missing interface
        r"
        import { Pipe } from '@angular/core';
        @Pipe({
            name: 'myPipe',
            standalone: true
        })
        class MyPipe {
            transform(value: any): any {
                return value;
            }
        }
        ",
        // Implements other interface but not PipeTransform
        r"
        import { Pipe, OnDestroy } from '@angular/core';
        @Pipe({
            name: 'myPipe'
        })
        class MyPipe implements OnDestroy {
            transform(value: any): any {
                return value;
            }
            ngOnDestroy() {}
        }
        ",
    ];

    Tester::new(UsePipeTransformInterface::NAME, UsePipeTransformInterface::PLUGIN, pass, fail)
        .test_and_snapshot();
}
