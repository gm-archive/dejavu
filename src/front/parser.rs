use std::mem;
use std::str::FromStr;

use symbol::{Symbol, keyword};
use front::{ast, Lexer, Span, ErrorHandler};
use front::token::{Token, Delim, BinOp};

pub struct Parser<'s, 'e> {
    reader: Lexer<'s>,
    errors: &'e ErrorHandler,

    current: Token,
    span: Span,
}

impl<'s, 'e> Parser<'s, 'e> {
    pub fn new(reader: Lexer<'s>, errors: &'e ErrorHandler) -> Parser<'s, 'e> {
        let mut parser = Parser {
            reader: reader,
            errors: errors,

            current: Token::Eof,
            span: Span { low: 0, high: 0 },
        };

        parser.advance_token();
        parser
    }

    pub fn parse_program(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (stmt, span) = if self.current == Token::OpenDelim(Delim::Brace) {
            self.parse_statement()
        } else {
            let mut stmts = vec![];
            let mut high = low;
            while self.current != Token::Eof {
                let (stmt, span) = self.parse_statement();
                stmts.push((stmt, span));
                high = span.high;
            }

            let span = Span { low: low, high: high };
            (ast::Stmt::Block(stmts.into_boxed_slice()), span)
        };
        let high = span.high;

        if self.current != Token::Eof {
            self.errors.error(self.span, "expected end of file");
        }

        (stmt, Span { low: low, high: high })
    }

    pub fn parse_statement(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;

        use front::token::Token::*;
        use symbol::keyword::*;

        #[allow(non_upper_case_globals)]
        let (stmt, span) = match self.current {
            Keyword(Var) | Keyword(GlobalVar) => self.parse_declare(),
            OpenDelim(Delim::Brace) | Keyword(Begin) => self.parse_block(),
            Keyword(If) => self.parse_if(),
            Keyword(Repeat) => self.parse_repeat(),
            Keyword(While) | Keyword(With) => self.parse_while_or_with(),
            Keyword(Do) => self.parse_do(),
            Keyword(For) => self.parse_for(),
            Keyword(Switch) => self.parse_switch(),
            Keyword(Break) | Keyword(Continue) | Keyword(Exit) => self.parse_jump(),
            Keyword(Return) => self.parse_return(),
            Keyword(Case) | Keyword(Default) => self.parse_case(),
            _ => self.parse_assign_or_invoke(),
        };

        let mut high = span.high;
        while self.current == Semicolon {
            high = self.span.high;
            self.advance_token();
        }

        (stmt, Span { low: low, high: high })
    }

    fn parse_assign_or_invoke(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (lvalue, left_span) = self.parse_term();

        match lvalue {
            ast::Expr::Call(call) => return (ast::Stmt::Invoke(call), left_span),
            _ => (),
        }

        use front::token::Token::*;
        use front::token::BinOp::*;
        use front::ast::Op::*;
        let op = match self.current {
            Eq | ColonEq => None,
            BinOpEq(Plus) => Some(Add),
            BinOpEq(Minus) => Some(Subtract),
            BinOpEq(Star) => Some(Multiply),
            BinOpEq(Slash) => Some(Divide),
            BinOpEq(Ampersand) => Some(BitAnd),
            BinOpEq(Pipe) => Some(BitOr),
            BinOpEq(Caret) => Some(BitXor),
            _ => {
                self.errors.error(self.span, "unexpected _; expected assignment operator");
                let (expr, expr_span) = self.parse_term();
                return (ast::Stmt::Error(expr), expr_span);
            }
        };
        self.advance_token();

        let (rvalue, right_span) = self.parse_expression(0);
        let high = right_span.high;

        let span = Span { low: low, high: high };
        (ast::Stmt::Assign(op, Box::new((lvalue, left_span)), Box::new((rvalue, right_span))), span)
    }

    fn parse_declare(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (declare, _) = self.advance_token();
        let declare = match declare {
            Token::Keyword(keyword::Var) => ast::Declare::Local,
            Token::Keyword(keyword::GlobalVar) => ast::Declare::Global,
            _ => unreachable!(),
        };

        let mut idents = vec![];
        while self.current != Token::Semicolon && self.current != Token::Eof {
            let (symbol, span) = match self.current {
                Token::Ident(symbol) => (symbol, self.span),
                _ => break,
            };

            idents.push((symbol, span));

            self.advance_token();
            if let Token::Comma = self.current {
                let _ = self.advance_token();
            }
        }

        let high = self.span.high;
        self.expect(Token::Semicolon);

        let span = Span { low: low, high: high };
        (ast::Stmt::Declare(declare, idents.into_boxed_slice()), span)
    }

    fn parse_block(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let mut stmts = vec![];
        while
            self.current != Token::CloseDelim(Delim::Brace) &&
            self.current != Token::Keyword(keyword::End) &&
            self.current != Token::Eof
        {
            let (stmt, span) = self.parse_statement();
            stmts.push((stmt, span));
        }

        let high;
        if self.current == Token::Eof {
            self.errors.error(self.span, "unexpected end of file; expected }");
            high = self.span.low;
        } else {
            let (_, span) = self.advance_token();
            high = span.high;
        }

        let span = Span { low: low, high: high };
        (ast::Stmt::Block(stmts.into_boxed_slice()), span)
    }

    fn parse_if(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let (expr, expr_span) = self.parse_expression(0);

        if self.current == Token::Keyword(keyword::Then) {
            self.advance_token();
        }

        let (true_branch, true_span) = self.parse_statement();

        let false_branch = if self.current == Token::Keyword(keyword::Else) {
            self.advance_token();
            Some(self.parse_statement())
        } else {
            None
        };
        let high = false_branch.as_ref().map(|&(_, span)| span.high).unwrap_or(true_span.high);

        let span = Span { low: low, high: high };
        (ast::Stmt::If(
            Box::new((expr, expr_span)),
            Box::new((true_branch, true_span)),
            false_branch.map(Box::new),
        ), span)
    }

    fn parse_repeat(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let (count, count_span) = self.parse_expression(0);
        let (body, body_span) = self.parse_statement();

        let high = body_span.high;

        let span = Span { low: low, high: high };
        (ast::Stmt::Repeat(Box::new((count, count_span)), Box::new((body, body_span))), span)
    }

    fn parse_while_or_with(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (kind, _) = self.advance_token();
        let kind = match kind {
            Token::Keyword(keyword::While) => ast::Stmt::While,
            Token::Keyword(keyword::With) => ast::Stmt::With,
            _ => unreachable!(),
        };

        let (expr, expr_span) = self.parse_expression(0);
        if self.current == Token::Keyword(keyword::Do) {
            self.advance_token();
        }
        let (body, body_span) = self.parse_statement();

        let high = body_span.high;

        let span = Span { low: low, high: high };
        (kind(Box::new((expr, expr_span)), Box::new((body, body_span))), span)
    }

    fn parse_do(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let (body, body_span) = self.parse_statement();
        self.expect(Token::Keyword(keyword::Until));
        let (expr, expr_span) = self.parse_expression(0);

        let high = expr_span.high;

        let span = Span { low: low, high: high };
        (ast::Stmt::Do(Box::new((body, body_span)), Box::new((expr, expr_span))), span)
    }

    fn parse_for(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        self.expect(Token::OpenDelim(Delim::Paren));

        let (init, init_span) = self.parse_statement();
        let (cond, cond_span) = self.parse_expression(0);
        if self.current == Token::Semicolon {
            self.advance_token();
        }
        let (next, next_span) = self.parse_statement();

        let high = self.span.high;
        self.expect(Token::CloseDelim(Delim::Paren));

        let (body, body_span) = self.parse_statement();

        let span = Span { low: low, high: high };
        (ast::Stmt::For(
            Box::new((init, init_span)),
            Box::new((cond, cond_span)),
            Box::new((next, next_span)),
            Box::new((body, body_span)),
        ), span)
    }

    fn parse_switch(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let (expr, expr_span) = self.parse_expression(0);

        if
            self.current != Token::OpenDelim(Delim::Brace) &&
            self.current != Token::Keyword(keyword::Begin)
        {
            self.errors.error(self.span, "unexpected _; expected {");
        }

        let (body, Span { high, .. }) = self.parse_block();
        let body = match body {
            ast::Stmt::Block(stmts) => stmts,
            _ => unreachable!(),
        };

        let span = Span { low: low, high: high };
        (ast::Stmt::Switch(Box::new((expr, expr_span)), body), span)
    }

    fn parse_jump(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (jump, Span { high, .. }) = self.advance_token();
        let jump = match jump {
            Token::Keyword(keyword::Break) => ast::Jump::Break,
            Token::Keyword(keyword::Continue) => ast::Jump::Continue,
            Token::Keyword(keyword::Exit) => ast::Jump::Exit,
            _ => unreachable!(),
        };

        let span = Span { low: low, high: high };
        (ast::Stmt::Jump(jump), span)
    }

    fn parse_return(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        self.advance_token();

        let (expr, expr_span) = self.parse_expression(0);
        let high = expr_span.high;

        let span = Span { low: low, high: high };
        (ast::Stmt::Return(Box::new((expr, expr_span))), span)
    }

    fn parse_case(&mut self) -> (ast::Stmt, Span) {
        let low = self.span.low;
        let (case, _) = self.advance_token();

        let expr = match case {
            Token::Keyword(keyword::Case) => Some(self.parse_expression(0)),
            Token::Keyword(keyword::Default) => None,
            _ => unreachable!(),
        };

        let high = self.span.high;
        self.expect(Token::Colon);

        let span = Span { low: low, high: high };
        (ast::Stmt::Case(expr.map(Box::new)), span)
    }

    fn parse_expression(&mut self, min_precedence: usize) -> (ast::Expr, Span) {
        let (mut left, mut left_span, mut parens) = self.parse_prefix_expression();
        while let Some((op, precedence)) = Infix::from_token(self.current) {
            if precedence < min_precedence {
                break;
            }

            let low = left_span.low;
            match (&left, op) {
                (&ast::Expr::Value(ast::Value::Ident(_)), Infix::Call) => {
                    let (args, high) = self.parse_args(Delim::Paren);

                    left = ast::Expr::Call(ast::Call(Box::new((left, left_span)), args));
                    left_span = Span { low: low, high: high };
                    parens = true;
                }

                (&ast::Expr::Value(ast::Value::Ident(_)), Infix::Index) |
                (&ast::Expr::Field(..), Infix::Index)
                if !parens => {
                    let (args, high) = self.parse_args(Delim::Bracket);

                    left = ast::Expr::Index(Box::new((left, left_span)), args);
                    left_span = Span { low: low, high: high };
                    parens = false;
                }

                (&ast::Expr::Value(ast::Value::Ident(_)), Infix::Field) |
                (&ast::Expr::Field(..), Infix::Field) |
                (&ast::Expr::Index(..), Infix::Field) => {
                    self.advance_token();

                    let (field, field_span) = if let Token::Ident(field) = self.current {
                        let (_, field_span) = self.advance_token();
                        (field, field_span)
                    } else {
                        self.errors.error(self.span, "unexpected _; expected identifier");
                        break;
                    };
                    let high = field_span.high;

                    left = ast::Expr::Field(Box::new((left, left_span)), (field, field_span));
                    left_span = Span { low: low, high: high };
                    parens = false;
                }

                (_, Infix::Binary(op)) => {
                    self.advance_token();

                    let (right, right_span) = self.parse_expression(precedence + 1);

                    left = ast::Expr::Binary(
                        op, Box::new((left, left_span)), Box::new((right, right_span))
                    );
                    left_span = Span { low: left_span.low, high: right_span.high };
                }

                _ => break,
            }
        }

        (left, left_span)
    }

    fn parse_prefix_expression(&mut self) -> (ast::Expr, Span, bool) {
        let low = self.span.low;

        use front::token::Token::*;
        use symbol::keyword::*;

        let (current, span) = self.advance_token();

        #[allow(non_upper_case_globals)]
        match current {
            Ident(symbol) | Keyword(symbol @ True) | Keyword(symbol @ False) |
            Keyword(symbol @ Self_) | Keyword(symbol @ Other) |
            Keyword(symbol @ All) | Keyword(symbol @ NoOne) |
            Keyword(symbol @ Global) | Keyword(symbol @ Local) => {
                (ast::Expr::Value(ast::Value::Ident(symbol)), span, false)
            }

            Real(symbol) => {
                let contents = symbol.as_str();
                let value = match contents.chars().next() {
                    Some('$') => u64::from_str_radix(&contents[1..], 16).unwrap_or_else(|_| {
                        self.errors.error(span, "invalid integer literal");
                        0
                    }) as f64,
                    _ => f64::from_str(&contents).unwrap_or_else(|_| {
                        self.errors.error(span, "invalid floating point literal");
                        0.0
                    }),
                };
                (ast::Expr::Value(ast::Value::Real(value)), span, false)
            }

            String(symbol) => {
                let contents = symbol.as_str();
                let symbol = Symbol::intern(&contents[1..contents.len() - 1]);
                (ast::Expr::Value(ast::Value::String(symbol)), span, false)
            }

            BinOp(self::BinOp::Plus) | BinOp(self::BinOp::Minus) | Bang | Keyword(Not) | Tilde => {
                let op = match current {
                    BinOp(self::BinOp::Plus) => ast::Unary::Positive,
                    BinOp(self::BinOp::Minus) => ast::Unary::Negate,
                    Bang | Keyword(Not) => ast::Unary::Invert,
                    Tilde => ast::Unary::BitInvert,
                    _ => unreachable!(),
                };

                let (expr, expr_span) = self.parse_term();
                let high = expr_span.high;

                let span = Span { low: low, high: high };
                (ast::Expr::Unary(op, Box::new((expr, expr_span))), span, true)
            }

            OpenDelim(Delim::Paren) => {
                let (expr, expr_span) = self.parse_expression(0);
                self.expect(CloseDelim(Delim::Paren));

                (expr, expr_span, true)
            }

            _ => {
                self.errors.error(self.span, "unexpected _; expected expression");

                let span = Span { low: low, high: low };
                (ast::Expr::Value(ast::Value::Real(0.0)), span, false)
            }
        }
    }

    fn parse_args(&mut self, delim: Delim) -> (Box<[(ast::Expr, Span)]>, usize) {
        self.advance_token();

        let mut args = vec![];
        while self.current != Token::CloseDelim(delim) && self.current != Token::Eof {
            args.push(self.parse_expression(0));

            if self.current == Token::Comma {
                self.advance_token();
            } else {
                break;
            }
        }

        let high = self.span.high;
        if self.current != Token::CloseDelim(delim) {
            self.errors.error(self.span, "unexpected _; expected _ or ,");
        } else {
            self.advance_token();
        }

        (args.into_boxed_slice(), high)
    }

    fn parse_term(&mut self) -> (ast::Expr, Span) {
        self.parse_expression(7)
    }

    fn expect(&mut self, token: Token) -> bool {
        if self.current == token {
            self.advance_token();
            true
        } else {
            self.errors.error(self.span, "unexpected _; expected _");
            false
        }
    }

    fn advance_token(&mut self) -> (Token, Span) {
        let (token, span) = self.reader.read_token();

        let token = mem::replace(&mut self.current, token);
        let span = mem::replace(&mut self.span, span);
        return (token, span);
    }
}

enum Infix {
    Binary(ast::Binary),
    Field,
    Index,
    Call,
}

impl Infix {
    fn from_token(token: Token) -> Option<(Infix, usize)> {
        use front::ast::Binary::*;
        use front::ast::Op::*;

        let op = match token {
            Token::Dot => Infix::Field,
            Token::OpenDelim(Delim::Bracket) => Infix::Index,
            Token::OpenDelim(Delim::Paren) => Infix::Call,

            _ => Infix::Binary(match token {
                Token::Lt => Lt,
                Token::Le => Le,
                Token::Eq => Eq,
                Token::EqEq => Eq,
                Token::Ne => Ne,
                Token::Ge => Ge,
                Token::Gt => Gt,
                Token::BinOp(op) => Op(from_binop(op)),
                Token::Keyword(keyword::Div) => Div,
                Token::Keyword(keyword::Mod) => Mod,
                Token::And | Token::Keyword(keyword::And) => And,
                Token::Or | Token::Keyword(keyword::Or) => Or,
                Token::Xor | Token::Keyword(keyword::Xor) => Xor,
                Token::Shl => ShiftLeft,
                Token::Shr => ShiftRight,

                _ => return None,
            }),
        };

        fn from_binop(op: BinOp) -> ast::Op {
            match op {
                BinOp::Plus => Add,
                BinOp::Minus => Subtract,
                BinOp::Star => Multiply,
                BinOp::Slash => Divide,
                BinOp::Ampersand => BitAnd,
                BinOp::Pipe => BitOr,
                BinOp::Caret => BitXor,
            }
        }

        let precedence = match op {
            Infix::Field | Infix::Index | Infix::Call => 7,
            Infix::Binary(op) => match op {
                Op(Multiply) | Op(Divide) | Div | Mod => 6,
                Op(Add) | Op(Subtract) => 5,
                ShiftLeft | ShiftRight => 4,
                Op(BitAnd) | Op(BitXor) | Op(BitOr) => 3,
                Lt | Le | Eq | Ne | Ge | Gt => 2,
                And | Or | Xor => 1,
            }
        };

        Some((op, precedence))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use symbol::Symbol;
    use front::{SourceFile, Span, ErrorHandler, Parser, Lexer};
    use front::ast::*;

    fn setup(source: &str) -> SourceFile {
        SourceFile {
            name: PathBuf::from("<test>"),
            source: String::from(source),
        }
    }

    fn span(low: usize, high: usize) -> Span {
        Span { low: low, high: high }
    }

    #[test]
    fn program() {
        let source = setup("{ \
            var x; \
            x = 3 \
            show_message(x * y) \
        }");
        let errors = ErrorHandler;
        let reader = Lexer::new(&source);
        let mut parser = Parser::new(reader, &errors);

        let x = Symbol::intern("x");
        let y = Symbol::intern("y");
        let show_message = Symbol::intern("show_message");
        assert_eq!(parser.parse_program(), (
            Stmt::Block(vec![
                (Stmt::Declare(
                    Declare::Local,
                    vec![(x, span(6, 7))].into_boxed_slice(),
                ), span(2, 8)), 
                (Stmt::Assign(
                    None,
                    Box::new((Expr::Value(Value::Ident(x)), span(9, 10))),
                    Box::new((Expr::Value(Value::Real(3.0)), span(13, 14))),
                ), span(9, 14)),
                (Stmt::Invoke(Call(
                    Box::new((Expr::Value(Value::Ident(show_message)), span(15, 27))),
                    vec![(Expr::Binary(
                        Binary::Op(Op::Multiply), 
                        Box::new((Expr::Value(Value::Ident(x)), span(28, 29))),
                        Box::new((Expr::Value(Value::Ident(y)), span(32, 33))),
                    ), span(28, 33))].into_boxed_slice(),
                )), span(15, 34)),
            ].into_boxed_slice()),
            span(0, 36),
        ));
    }

    #[test]
    fn precedence() {
        let source = setup("x + y * (3 + z)");
        let errors = ErrorHandler;
        let reader = Lexer::new(&source);
        let mut parser = Parser::new(reader, &errors);

        let x = Symbol::intern("x");
        let y = Symbol::intern("y");
        let z = Symbol::intern("z");
        assert_eq!(parser.parse_expression(0), (
            Expr::Binary(
                Binary::Op(Op::Add),
                Box::new((Expr::Value(Value::Ident(x)), span(0, 1))),
                Box::new((Expr::Binary(
                    Binary::Op(Op::Multiply),
                    Box::new((Expr::Value(Value::Ident(y)), span(4, 5))),
                    Box::new((Expr::Binary(
                        Binary::Op(Op::Add),
                        Box::new((Expr::Value(Value::Real(3.0)), span(9, 10))),
                        Box::new((Expr::Value(Value::Ident(z)), span(13, 14))),
                    ), span(9, 14))),
                ), span(4, 14))),
            ),
            span(0, 14)
        ));
    }
}
