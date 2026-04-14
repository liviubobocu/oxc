use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        get_component_metadata, get_decorator_name
}
};

fn component_max_inline_declarations_diagnostic(
    span: Span,
    property: &str,
    actual: usize,
    max: usize,
) -> OxcDiagnostic {
    // Match ESLint message format exactly: `template` has too many lines (4). Maximum allowed is 3
    OxcDiagnostic::warn(format!(
        "`{property}` has too many lines ({actual}). Maximum allowed is {max}"
    ))
    .with_help(format!(
        "Extract the inline {property} to a separate file. For templates use `templateUrl`, \
        for styles use `styleUrl` or `styleUrls`."
    ))
    .with_label(span)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ComponentMaxInlineDeclarationsConfig {
    /// Maximum lines for inline template (default: 3)
    #[serde(default = "default_template")]
    template: usize,
    /// Maximum lines for inline styles (default: 3)
    #[serde(default = "default_styles")]
    styles: usize,
    /// Maximum lines for inline animations (default: 15)
    #[serde(default = "default_animations")]
    animations: usize
}

fn default_template() -> usize {
    3
}

fn default_styles() -> usize {
    3
}

fn default_animations() -> usize {
    15
}

impl Default for ComponentMaxInlineDeclarationsConfig {
    fn default() -> Self {
        Self {
            template: default_template(),
            styles: default_styles(),
            animations: default_animations()
}
    }
}

#[derive(Debug, Clone)]
#[expect(clippy::struct_field_names)]
pub struct ComponentMaxInlineDeclarations {
    template_max: usize,
    styles_max: usize,
    animations_max: usize
}

impl Default for ComponentMaxInlineDeclarations {
    fn default() -> Self {
        Self {
            template_max: default_template(),
            styles_max: default_styles(),
            animations_max: default_animations()
}
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a maximum number of lines for inline templates, styles, and animations in components.
    ///
    /// ### Why is this bad?
    ///
    /// Large inline templates, styles, or animations make components harder to read and maintain.
    /// When these become too long, they should be extracted to separate files:
    /// - Templates: Use `templateUrl` instead of `template`
    /// - Styles: Use `styleUrl` or `styleUrls` instead of `styles`
    /// - Animations: Consider extracting to a separate animations file
    ///
    /// ### Configuration
    ///
    /// ```json
    /// {
    ///   "angular/component-max-inline-declarations": ["error", {
    ///     "template": 3,
    ///     "styles": 3,
    ///     "animations": 15
    ///   }]
    /// }
    /// ```
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with default config):
    /// ```typescript
    /// @Component({
    ///   selector: 'app-example',
    ///   template: `
    ///     <div>Line 1</div>
    ///     <div>Line 2</div>
    ///     <div>Line 3</div>
    ///     <div>Line 4</div>
    ///   `
    /// })
    /// export class ExampleComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```typescript
    /// @Component({
    ///   selector: 'app-example',
    ///   templateUrl: './example.component.html'
    /// })
    /// export class ExampleComponent {}
    /// ```
    ComponentMaxInlineDeclarations,
    angular,
    style,
    pending
);

impl Rule for ComponentMaxInlineDeclarations {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config_value = value.get(0).unwrap_or(&value);
        let config: ComponentMaxInlineDeclarationsConfig =
            serde_json::from_value(config_value.clone())?;
        Ok(Self {
            template_max: config.template,
            styles_max: config.styles,
            animations_max: config.animations
})
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else {
            return;
        };

        // Only check @Component decorator
        let Some(decorator_name) = get_decorator_name(decorator) else {
            return;
        };

        if decorator_name != "Component" {
            return;
        }
        // Note: Match ESLint behavior - does not verify imports for exact parity
        // Get the metadata object
        let Some(metadata) = get_component_metadata(decorator) else {
            return;
        };

        // Check template
        if let Some((span, line_count)) = get_template_line_count(metadata)
            && line_count > self.template_max {
                ctx.diagnostic(component_max_inline_declarations_diagnostic(
                    span,
                    "template",
                    line_count,
                    self.template_max,
                ));
            }

        // Check styles (array or string)
        if let Some((span, line_count)) = get_styles_line_count(metadata)
            && line_count > self.styles_max {
                ctx.diagnostic(component_max_inline_declarations_diagnostic(
                    span,
                    "styles",
                    line_count,
                    self.styles_max,
                ));
            }

        // Check animations (array only, uses line-based counting)
        if let Some((span, line_count)) = get_animations_line_count(metadata, ctx)
            && line_count > self.animations_max {
                ctx.diagnostic(component_max_inline_declarations_diagnostic(
                    span,
                    "animations",
                    line_count,
                    self.animations_max,
                ));
            }
    }
}

/// Get line count for template property.
/// ESLint counts lines by trimming the content and splitting by newlines.
fn get_template_line_count(
    metadata: &oxc_ast::ast::ObjectExpression<'_>,
) -> Option<(Span, usize)> {
    for prop in &metadata.properties {
        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop {
            let prop_name = match &obj_prop.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
                oxc_ast::ast::PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
                _ => None
            };

            if prop_name == Some("template") {
                let line_count = get_lines_count(&obj_prop.value);
                return Some((obj_prop.value.span(), line_count));
            }
        }
    }
    None
}

/// Get line count for styles property.
/// Styles can be an array or a single string. For arrays, sum lines of all elements.
fn get_styles_line_count(metadata: &oxc_ast::ast::ObjectExpression<'_>) -> Option<(Span, usize)> {
    for prop in &metadata.properties {
        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop {
            let prop_name = match &obj_prop.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
                oxc_ast::ast::PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
                _ => None
            };

            if prop_name == Some("styles") {
                let line_count = match &obj_prop.value {
                    oxc_ast::ast::Expression::ArrayExpression(array) => {
                        // Sum lines across all elements
                        array
                            .elements
                            .iter()
                            .filter_map(|el| el.as_expression().map(get_lines_count))
                            .sum()
                    }
                    expr => get_lines_count(expr)
                };
                return Some((obj_prop.value.span(), line_count));
            }
        }
    }
    None
}

/// Get line count for animations property.
/// ESLint uses source location-based counting: end.line - start.line - 2 (for brackets), min 1.
/// Only works for array expressions with elements.
fn get_animations_line_count(
    metadata: &oxc_ast::ast::ObjectExpression<'_>,
    ctx: &LintContext<'_>,
) -> Option<(Span, usize)> {
    for prop in &metadata.properties {
        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(obj_prop) = prop {
            let prop_name = match &obj_prop.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
                oxc_ast::ast::PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
                _ => None
            };

            if prop_name == Some("animations") {
                // ESLint: only check array expressions with elements
                if let oxc_ast::ast::Expression::ArrayExpression(array) = &obj_prop.value {
                    if array.elements.is_empty() {
                        return None;
                    }

                    // Calculate line count using source location like ESLint:
                    // lineCount = end.line - start.line - animationsBracketsSize (2)
                    let span = obj_prop.value.span();
                    let start_line = ctx.source_text()[..span.start as usize]
                        .chars()
                        .filter(|&c| c == '\n')
                        .count() + 1;
                    let end_line = ctx.source_text()[..span.end as usize]
                        .chars()
                        .filter(|&c| c == '\n')
                        .count() + 1;

                    let animations_brackets_size = 2;
                    let line_diff = end_line.saturating_sub(start_line);
                    let line_count = if line_diff >= animations_brackets_size {
                        line_diff - animations_brackets_size
                    } else {
                        0
                    };
                    let line_count = line_count.max(1);

                    return Some((span, line_count));
                }
                // Not an array expression - skip (matches ESLint behavior)
                return None;
            }
        }
    }
    None
}

/// Count lines in a literal expression matching ESLint behavior.
/// For template literals: trim quasis[0].value.raw and split by newlines.
/// For string literals: trim raw value and split by newlines.
fn get_lines_count(expr: &oxc_ast::ast::Expression<'_>) -> usize {
    match expr {
        oxc_ast::ast::Expression::TemplateLiteral(lit) => {
            // ESLint: node.quasis[0].value.raw.trim().split(NEW_LINE_REGEXP).length
            if let Some(first_quasi) = lit.quasis.first() {
                let raw = first_quasi.value.raw.as_str();
                count_trimmed_lines(raw)
            } else {
                0
            }
        }
        oxc_ast::ast::Expression::StringLiteral(lit) => {
            // ESLint: node.raw.trim().split(NEW_LINE_REGEXP).length
            // The raw value includes quotes, but for single-line strings this works the same
            // For our purposes, we use the value and handle it similarly
            count_trimmed_lines(lit.value.as_str())
        }
        _ => 0
    }
}

/// Count lines by trimming and splitting on newlines.
/// Matches ESLint's: str.trim().split(/\r\n|\r|\n/).length
/// Note: JavaScript's split() always returns at least 1 element for non-matching splits
fn count_trimmed_lines(s: &str) -> usize {
    let trimmed = s.trim();
    // JavaScript "".split(/\n/) returns [""], length 1
    // JavaScript "foo".split(/\n/) returns ["foo"], length 1
    // JavaScript "foo\nbar".split(/\n/) returns ["foo", "bar"], length 2
    // So even empty string after trim counts as 1 line in JavaScript
    if trimmed.is_empty() {
        return 1;
    }
    // Split by any newline variant - lines() handles \r\n, \r, \n
    let count = trimmed.lines().count();
    // lines() may return 0 for some edge cases, but with non-empty trimmed we know it's at least 1
    count.max(1)
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        // should succeed if the number of the template lines does not exceed the default lines limit
        (
            r"
    @Component({
      template: '<div>just one line template</div>'
    })
    class Test {}
    ",
            None,
        ),
        // should succeed if the number of the styles lines does not exceed the default lines limit
        (
            r"
    @Component({
      styles: ['div { display: none; }']
    })
    class Test {}
    ",
            None,
        ),
        // should succeed if the number of the styles lines does not exceed the default lines limit with a string value
        (
            r"
    @Component({
      styles: 'div { display: none; }'
    })
    class Test {}
    ",
            None,
        ),
        // should succeed if the number of the animations lines does not exceed the default lines limit
        (
            r"
    @Component({
      animations: [state('void', style({opacity: 0, transform: 'scale(1, 0)'}))]
    })
    class Test {}
    ",
            None,
        ),
        // should succeed if template, styles and animations properties are not present
        (
            r"
    @Component({
      styleUrls: ['./foobar.scss'],
      templateUrl: './foobar.html',
    })
    class Test {}
    ",
            None,
        ),
        // should succeed with animations within limit
        (
            r"
    @Component({
      animations: [
        state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
      ],
      templateUrl: './foobar.html',
    })
    class Test {}
    ",
            None,
        ),
    ];

    let fail = vec![
        // should fail if the number of the template lines exceeds the default lines limit
        (
            r"
      @Component({
        template: `

          <div>first line</div>
          <div>second line</div>
          <div>third line</div>
          <div>fourth line</div>
        `

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the number of lines exceeds a custom lines limit (template)
        (
            r"
      @Component({
        template: '<div>first line</div>'

      })
      class Test {}
      ",
            Some(serde_json::json!([{ "template": 0 }])),
        ),
        // should fail if the number of the styles lines exceeds the default lines limit
        (
            r"
      @Component({
        styles: [

          `
            div {
              display: block;
              height: 40px;
            }
          `
        ]

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the number of the styles lines exceeds the default lines limit with a string value
        (
            r"
      @Component({
        styles: `

          div {
            display: block;
            height: 40px;
          }
        `

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the sum of lines (from separate styles) exceeds the default lines limit
        (
            r"
      @Component({
        styles: [

          `
            div {
              display: block;
            }
          `,
          `
            span {
              width: 30px;
            }
          `
        ]

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the number of the styles lines exceeds a custom lines limit
        (
            r"
      @Component({
        styles: ['div { display: none; }']

      })
      class Test {}
      ",
            Some(serde_json::json!([{ "styles": 0 }])),
        ),
        // should fail if the number of the animations lines exceeds the default lines limit
        (
            r"
      @Component({
        animations: [{

          transformPanelWrap: trigger('transformPanelWrap', [
            transition('* => void', query('@transformPanel', [animateChild()], {optional: true})),
          ]),
          transformPanel: trigger('transformPanel', [
            state('void', style({
              transform: 'scaleY(0.8)',
              minWidth: '100%',
              opacity: 0
            })),
            state('showing', style({
              opacity: 1,
              minWidth: 'calc(100% + 32px)',
              transform: 'scaleY(1)'
            })),
            state('next', style({height: '0px', visibility: 'hidden'}))
          ])
        }]

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the sum of lines (from separate animations) exceeds the default lines limit
        (
            r"
      @Component({
        animations: [

          trigger('dialogContainer', [
            transition('* => void', query('@transformPanel', [animateChild()], {optional: true}))
          ]),
          trigger('transformPanel', [
            state('void', style({
              transform: 'scaleY(0.8)',
              minWidth: '100%',
              opacity: 0
            })),
            state('showing', style({
              opacity: 1,
              minWidth: 'calc(100% + 32px)',
              transform: 'scaleY(1)'
            }))
          ]),
          trigger('transformPanel', [
            state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
          ])
        ]

      })
      class Test {}
      ",
            None,
        ),
        // should fail if the number of the animations lines exceeds a custom lines limit
        (
            r"
      @Component({
        animations: [{

          transformPanel: trigger('transformPanel', [
            state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
          ])
        }]

      })
      class Test {}
      ",
            Some(serde_json::json!([{ "animations": 2 }])),
        ),
    ];

    Tester::new(
        ComponentMaxInlineDeclarations::NAME,
        ComponentMaxInlineDeclarations::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
