use std::collections::HashMap;

use codemap::{Span, Spanned};

use peekmore::PeekMoreIterator;

use crate::error::SassResult;
use crate::scope::Scope;
use crate::selector::Selector;
use crate::utils::{
    devour_whitespace, devour_whitespace_or_comment, eat_ident, eat_ident_no_interpolation,
    read_until_closing_paren, read_until_closing_quote, read_until_closing_square_brace,
};
use crate::value::Value;
use crate::Token;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct FuncArgs(pub Vec<FuncArg>);

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct FuncArg {
    pub name: String,
    pub default: Option<Vec<Token>>,
    pub is_variadic: bool,
}

impl FuncArgs {
    pub const fn new() -> Self {
        FuncArgs(Vec::new())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallArgs(HashMap<CallArg, Vec<Token>>, Span);

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
enum CallArg {
    Named(String),
    Positional(usize),
}

impl CallArg {
    pub fn position(&self) -> Result<usize, String> {
        match self {
            Self::Named(ref name) => Err(name.clone()),
            Self::Positional(p) => Ok(*p),
        }
    }

    pub fn decrement(self) -> CallArg {
        match self {
            Self::Named(..) => self,
            Self::Positional(p) => Self::Positional(p - 1),
        }
    }
}

impl CallArgs {
    pub fn new(span: Span) -> Self {
        CallArgs(HashMap::new(), span)
    }

    pub fn to_css_string(
        self,
        scope: &Scope,
        super_selector: &Selector,
    ) -> SassResult<Spanned<String>> {
        let mut string = String::with_capacity(2 + self.len() * 10);
        string.push('(');
        let mut span = self.1;

        if self.is_empty() {
            return Ok(Spanned {
                node: "()".to_string(),
                span,
            });
        }

        let args = match self.get_variadic(scope, super_selector) {
            Ok(v) => v,
            Err(..) => {
                return Err(("Plain CSS functions don't support keyword arguments.", span).into())
            }
        };

        string.push_str(
            &args
                .iter()
                .map(|a| {
                    span = span.merge(a.span);
                    Ok(a.node.to_css_string(a.span)?)
                })
                .collect::<SassResult<Vec<String>>>()?
                .join(", "),
        );
        string.push(')');
        Ok(Spanned { node: string, span })
    }

    /// Get argument by name
    ///
    /// Removes the argument
    pub fn get_named(
        &mut self,
        val: String,
        scope: &Scope,
        super_selector: &Selector,
    ) -> Option<SassResult<Spanned<Value>>> {
        match self.0.remove(&CallArg::Named(val)) {
            Some(v) => Some(Value::from_vec(v, scope, super_selector)),
            None => None,
        }
    }

    /// Get a positional argument by 0-indexed position
    ///
    /// Removes the argument
    pub fn get_positional(
        &mut self,
        val: usize,
        scope: &Scope,
        super_selector: &Selector,
    ) -> Option<SassResult<Spanned<Value>>> {
        match self.0.remove(&CallArg::Positional(val)) {
            Some(v) => Some(Value::from_vec(v, scope, super_selector)),
            None => None,
        }
    }

    pub fn get_variadic(
        self,
        scope: &Scope,
        super_selector: &Selector,
    ) -> SassResult<Vec<Spanned<Value>>> {
        let mut vals = Vec::new();
        let mut args = match self
            .0
            .into_iter()
            .map(|(a, v)| Ok((a.position()?, v)))
            .collect::<Result<Vec<(usize, Vec<Token>)>, String>>()
        {
            Ok(v) => v,
            Err(e) => return Err((format!("No argument named ${}.", e), self.1).into()),
        };
        args.sort_by(|(a1, _), (a2, _)| a1.cmp(a2));
        for arg in args {
            vals.push(Value::from_vec(arg.1, scope, super_selector)?);
        }
        Ok(vals)
    }

    pub fn decrement(self) -> Self {
        CallArgs(
            self.0
                .into_iter()
                .map(|(k, v)| (k.decrement(), v))
                .collect(),
            self.1,
        )
    }

    pub const fn span(&self) -> Span {
        self.1
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn max_args(&self, max: usize) -> SassResult<()> {
        let len = self.len();
        if len > max {
            let mut err = String::with_capacity(50);
            err.push_str(&format!("Only {} argument", max));
            if max != 1 {
                err.push('s');
            }
            err.push_str(" allowed, but ");
            err.push_str(&len.to_string());
            err.push(' ');
            if len == 1 {
                err.push_str("was passed.")
            } else {
                err.push_str("were passed.")
            }
            return Err((err, self.span()).into());
        }
        Ok(())
    }
}

pub(crate) fn eat_func_args<I: Iterator<Item = Token>>(
    toks: &mut PeekMoreIterator<I>,
    scope: &Scope,
    super_selector: &Selector,
) -> SassResult<FuncArgs> {
    let mut args: Vec<FuncArg> = Vec::new();

    devour_whitespace(toks);
    while let Some(Token { kind, .. }) = toks.next() {
        let name = match kind {
            '$' => eat_ident(toks, scope, super_selector)?,
            ')' => break,
            _ => todo!(),
        };
        let mut default: Vec<Token> = Vec::new();
        let mut is_variadic = false;
        devour_whitespace(toks);
        let (kind, span) = match toks.next() {
            Some(Token { kind, pos }) => (kind, pos),
            _ => todo!("unexpected eof"),
        };
        match kind {
            ':' => {
                devour_whitespace(toks);
                while let Some(tok) = toks.peek() {
                    match &tok.kind {
                        ',' => {
                            toks.next();
                            args.push(FuncArg {
                                name: name.replace('_', "-"),
                                default: Some(default),
                                is_variadic,
                            });
                            break;
                        }
                        ')' => {
                            args.push(FuncArg {
                                name: name.replace('_', "-"),
                                default: Some(default),
                                is_variadic,
                            });
                            break;
                        }
                        _ => {
                            let tok = toks.next().expect("we know this exists!");
                            default.push(tok)
                        }
                    }
                }
            }
            '.' => {
                let next = toks.next().ok_or(("expected \".\".", span))?;
                if next.kind != '.' {
                    return Err(("expected \".\".", next.pos()).into());
                }
                let next = toks.next().ok_or(("expected \".\".", next.pos()))?;
                if next.kind != '.' {
                    return Err(("expected \".\".", next.pos()).into());
                }
                devour_whitespace(toks);
                let next = toks.next().ok_or(("expected \")\".", next.pos()))?;
                if next.kind != ')' {
                    return Err(("expected \")\".", next.pos()).into());
                }

                is_variadic = true;

                args.push(FuncArg {
                    name: name.replace('_', "-"),
                    default: Some(default),
                    is_variadic,
                });
                break;
            }
            ')' => {
                args.push(FuncArg {
                    name: name.replace('_', "-"),
                    default: if default.is_empty() {
                        None
                    } else {
                        Some(default)
                    },
                    is_variadic,
                });
                break;
            }
            ',' => args.push(FuncArg {
                name: name.replace('_', "-"),
                default: None,
                is_variadic,
            }),
            _ => {}
        }
        devour_whitespace(toks);
    }
    devour_whitespace(toks);
    if let Some(Token { kind: '{', .. }) = toks.next() {
    } else {
        todo!("expected `{{` after args")
    }
    Ok(FuncArgs(args))
}

pub(crate) fn eat_call_args<I: Iterator<Item = Token>>(
    toks: &mut PeekMoreIterator<I>,
) -> SassResult<CallArgs> {
    let mut args: HashMap<CallArg, Vec<Token>> = HashMap::new();
    devour_whitespace_or_comment(toks)?;
    let mut name = String::new();
    let mut val: Vec<Token> = Vec::new();
    // todo: panics on a { color:rgb(; }
    let mut span = toks.peek().unwrap().pos();
    loop {
        match toks.peek().unwrap().kind {
            '$' => {
                let Token { pos, .. } = toks.next().unwrap();
                let v = eat_ident_no_interpolation(toks, false)?;
                let whitespace = devour_whitespace_or_comment(toks)?;
                if toks.peek().unwrap().kind == ':' {
                    toks.next();
                    name = v.node;
                } else {
                    val.push(Token::new(pos, '$'));
                    let mut current_pos = 0;
                    val.extend(v.chars().map(|x| {
                        let len = x.len_utf8() as u64;
                        let tok = Token::new(v.span.subspan(current_pos, current_pos + len), x);
                        current_pos += len;
                        tok
                    }));
                    if whitespace {
                        val.push(Token::new(pos, ' '));
                    }
                    name.clear();
                }
            }
            ')' => {
                toks.next();
                return Ok(CallArgs(args, span));
            }
            _ => name.clear(),
        }
        devour_whitespace_or_comment(toks)?;

        while let Some(tok) = toks.next() {
            match tok.kind {
                ')' => {
                    args.insert(
                        if name.is_empty() {
                            CallArg::Positional(args.len())
                        } else {
                            CallArg::Named(name.replace('_', "-"))
                        },
                        val,
                    );
                    span = span.merge(tok.pos());
                    return Ok(CallArgs(args, span));
                }
                ',' => break,
                '[' => {
                    val.push(tok);
                    val.extend(read_until_closing_square_brace(toks));
                }
                '(' => {
                    val.push(tok);
                    val.extend(read_until_closing_paren(toks));
                }
                '"' | '\'' => {
                    val.push(tok);
                    val.extend(read_until_closing_quote(toks, tok.kind));
                }
                _ => val.push(tok),
            }
        }

        args.insert(
            if name.is_empty() {
                CallArg::Positional(args.len())
            } else {
                CallArg::Named(name.replace('_', "-"))
            },
            val.clone(),
        );
        val.clear();
        devour_whitespace(toks);

        if toks.peek().is_none() {
            return Ok(CallArgs(args, span));
        }
    }
}
