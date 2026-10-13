use crate::scripting::ast::*;

pub(super) fn compile_function(function: &FunctionDecl) -> CompiledFunction {
    let mut compiler = FunctionCompiler {
        instructions: Vec::new(),
    };

    compiler.compile_block(&function.body);

    CompiledFunction {
        instructions: compiler.instructions,
    }
}

/// Compiles one event body into a resumable execution plan.
pub(super) fn compile_event(event: &EventDecl) -> CompiledFunction {
    let mut compiler = FunctionCompiler {
        instructions: Vec::new(),
    };

    compiler.compile_block(&event.body);

    CompiledFunction {
        instructions: compiler.instructions,
    }
}

/// Compiles top-level statements into a resumable execution plan.
pub(super) fn compile_top_level(statements: &[Statement]) -> CompiledFunction {
    let mut compiler = FunctionCompiler {
        instructions: Vec::new(),
    };

    for statement in statements {
        compiler.compile_statement(statement);
    }

    CompiledFunction {
        instructions: compiler.instructions,
    }
}

pub(super) struct FunctionCompiler {
    pub(super) instructions: Vec<Instruction>,
}

impl FunctionCompiler {
    fn emit(&mut self, instruction: Instruction) -> usize {
        let index = self.instructions.len();

        self.instructions.push(instruction);

        index
    }

    pub(super) fn compile_block(&mut self, block: &Block) {
        self.emit(Instruction::EnterScope);

        for statement in &block.statements {
            self.compile_statement(statement);
        }

        self.emit(Instruction::ExitScope);
    }

    fn compile_statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Variable {
                name,
                initializer,
                is_const,
                ..
            } => {
                self.emit(Instruction::Variable {
                    name: name.clone(),
                    initializer: initializer.clone(),
                    is_const: *is_const,
                });
            }

            StatementKind::Assignment {
                target,
                operator,
                value,
            } => {
                self.emit(Instruction::Assignment {
                    target: target.clone(),
                    operator: *operator,
                    value: value.clone(),
                });
            }

            StatementKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                self.compile_if(condition, then_block, else_if, else_block);
            }

            StatementKind::While { condition, body } => {
                self.compile_while(condition, body);
            }

            StatementKind::For {
                name,
                iterable,
                body,
            } => {
                self.compile_for(name, iterable, body);
            }

            StatementKind::Return(value) => {
                self.emit(Instruction::Return(value.clone()));
            }

            StatementKind::Expression(expression) => {
                // Worldkiln 0.8.7 | WO 0.8.7-25 | 2026-10-06 15:03 -05:00
                // Keep the original call span when lowering standalone calls/waits.
                // Deriving a VM source point from the first argument loses zero-arg
                // calls and shifts wait() breakpoints onto the argument expression.
                if let Some(arguments) = standalone_wait_arguments(expression) {
                    self.emit(Instruction::Wait {
                        arguments,
                        span: expression.span,
                    });
                } else if let Some((name, arguments)) = standalone_call(expression) {
                    self.emit(Instruction::CallUserFunction {
                        name,
                        arguments,
                        span: expression.span,
                    });
                } else {
                    self.emit(Instruction::Evaluate(expression.clone()));
                }
            }
        }
    }

    fn compile_if(
        &mut self,
        condition: &Expression,
        then_block: &Block,
        else_if: &[(Expression, Block)],
        else_block: &Option<Block>,
    ) {
        let first_condition_jump = self.emit(Instruction::JumpIfFalse {
            condition: condition.clone(),
            target: 0,
        });

        self.compile_block(then_block);

        let mut end_jumps = Vec::new();

        end_jumps.push(self.emit(Instruction::Jump { target: 0 }));

        let mut next_condition = self.instructions.len();

        self.patch_jump_if_false(first_condition_jump, next_condition);

        for (condition, block) in else_if {
            let condition_jump = self.emit(Instruction::JumpIfFalse {
                condition: condition.clone(),
                target: 0,
            });

            self.compile_block(block);

            end_jumps.push(self.emit(Instruction::Jump { target: 0 }));

            next_condition = self.instructions.len();

            self.patch_jump_if_false(condition_jump, next_condition);
        }

        if let Some(block) = else_block {
            self.compile_block(block);
        }

        let end = self.instructions.len();

        for jump in end_jumps {
            self.patch_jump(jump, end);
        }
    }

    fn compile_while(&mut self, condition: &Expression, body: &Block) {
        let loop_start = self.instructions.len();

        let condition_jump = self.emit(Instruction::JumpIfFalse {
            condition: condition.clone(),
            target: 0,
        });

        self.compile_block(body);

        self.emit(Instruction::Jump { target: loop_start });

        let end = self.instructions.len();

        self.patch_jump_if_false(condition_jump, end);
    }

    fn compile_for(&mut self, name: &str, iterable: &Expression, body: &Block) {
        // The loop-variable scope remains alive across all iterations.
        self.emit(Instruction::EnterScope);

        let init = self.emit(Instruction::ForInit {
            name: name.to_string(),
            iterable: iterable.clone(),
            end: 0,
        });

        let body_start = self.instructions.len();

        self.compile_block(body);

        let next = self.emit(Instruction::ForNext { body_start, end: 0 });

        let exit_scope = self.emit(Instruction::ExitScope);

        self.patch_for_init(init, exit_scope);

        self.patch_for_next(next, exit_scope);
    }

    fn patch_jump(&mut self, index: usize, target: usize) {
        match &mut self.instructions[index] {
            Instruction::Jump {
                target: jump_target,
            } => {
                *jump_target = target;
            }

            _ => {
                panic!("AeoScript compiler attempted to patch a non-jump instruction");
            }
        }
    }

    fn patch_jump_if_false(&mut self, index: usize, target: usize) {
        match &mut self.instructions[index] {
            Instruction::JumpIfFalse {
                target: jump_target,
                ..
            } => {
                *jump_target = target;
            }

            _ => {
                panic!("AeoScript compiler attempted to patch a non-conditional jump");
            }
        }
    }

    fn patch_for_init(&mut self, index: usize, target: usize) {
        match &mut self.instructions[index] {
            Instruction::ForInit { end, .. } => {
                *end = target;
            }

            _ => {
                panic!("AeoScript compiler attempted to patch a non-for-init instruction");
            }
        }
    }

    fn patch_for_next(&mut self, index: usize, target: usize) {
        match &mut self.instructions[index] {
            Instruction::ForNext { end, .. } => {
                *end = target;
            }

            _ => {
                panic!("AeoScript compiler attempted to patch a non-for-next instruction");
            }
        }
    }
}

fn standalone_call(expression: &Expression) -> Option<(String, Vec<Expression>)> {
    match &expression.kind {
        ExpressionKind::Call { callee, arguments } => match &callee.kind {
            ExpressionKind::Identifier(name) => Some((name.clone(), arguments.clone())),

            _ => None,
        },

        _ => None,
    }
}

fn standalone_wait_arguments(expression: &Expression) -> Option<Vec<Expression>> {
    match &expression.kind {
        ExpressionKind::Call { callee, arguments } => match &callee.kind {
            ExpressionKind::Identifier(name) if name == "wait" => Some(arguments.clone()),

            _ => None,
        },

        _ => None,
    }
}
