using System.Collections.Generic;
using Mono.Cecil;
using Mono.Cecil.Cil;
using Mono.Cecil.Rocks;

namespace SpecGate.Weaver;

/// <summary>
/// Rewrites one annotated method so it opens a capture scope, records its
/// inputs, and reports a terminal status on every exit path.
/// </summary>
/// <remarks>
/// The woven shape is:
/// <code>
/// scope = EnterOperation(ldtoken thisMethod, operation, component);
/// RecordInputs(scope, new object[] { a, b });
/// try { ...original body, every `ret` becomes `leave epilogue`... }
/// catch (Exception e) { ExitError(scope, e); throw; }
/// epilogue: ExitOk(scope, result); return result;
/// </code>
/// The catch rethrows, so instrumentation never changes observable behaviour.
/// </remarks>
internal sealed class MethodWeaver(ModuleDefinition module, RuntimeReferences runtime)
{
    internal void Weave(MethodDefinition method, OperationDeclaration declaration)
    {
        MethodBody body = method.Body;
        body.SimplifyMacros();
        body.InitLocals = true;

        ILProcessor il = body.GetILProcessor();
        TypeReference exceptionType = module.ImportReference(runtime.ExitError.Parameters[1].ParameterType);

        VariableDefinition scope = AddVariable(body, runtime.ScopeType);
        VariableDefinition error = AddVariable(body, exceptionType);
        bool returnsValue = method.ReturnType.MetadataType != MetadataType.Void;
        VariableDefinition? result = returnsValue ? AddVariable(body, method.ReturnType) : null;

        Instruction firstOriginal = body.Instructions[0];
        foreach (Instruction instruction in BuildPrologue(il, method, declaration, scope))
        {
            il.InsertBefore(firstOriginal, instruction);
        }

        List<Instruction> epilogue = BuildEpilogue(il, method, scope, result);
        Instruction epilogueStart = epilogue[0];
        RedirectReturns(il, body, epilogueStart, result);

        List<Instruction> handler =
        [
            il.Create(OpCodes.Stloc, error),
            il.Create(OpCodes.Ldloc, scope),
            il.Create(OpCodes.Ldloc, error),
            il.Create(OpCodes.Call, runtime.ExitError),
            il.Create(OpCodes.Rethrow),
        ];

        foreach (Instruction instruction in handler)
        {
            il.Append(instruction);
        }

        foreach (Instruction instruction in epilogue)
        {
            il.Append(instruction);
        }

        body.ExceptionHandlers.Add(new ExceptionHandler(ExceptionHandlerType.Catch)
        {
            CatchType = exceptionType,
            TryStart = firstOriginal,
            TryEnd = handler[0],
            HandlerStart = handler[0],
            HandlerEnd = epilogueStart,
        });

        body.OptimizeMacros();
    }

    /// <summary>
    /// Rewrites one setup producer so it commits construction provenance on its
    /// success path.
    /// </summary>
    /// <remarks>
    /// The woven shape carries no scope and no handler:
    /// <code>
    /// staged = StageSetup(ldtoken thisMethod, new object[] { a, b });
    /// ...original body, every `ret` becomes `leave epilogue`...
    /// epilogue: CommitSetup(staged); return result;
    /// </code>
    /// Inputs are projected at entry, mirroring <c>defer_setup</c>
    /// (<c>rust/crates/specgate-runtime/src/capture/setup.rs:55-67</c>), so a body
    /// that mutates its arguments cannot change what is recorded. The original
    /// <c>ret</c> instructions are the only way to reach the epilogue, so a setup
    /// that throws commits nothing — which is Rust's "commit only after a
    /// successful return" without needing a handler. A rejection raised by
    /// <c>CommitSetup</c> is raised outside any try region and reaches the
    /// caller, matching the rule that a capture rejection is a defect to surface.
    /// </remarks>
    internal void WeaveSetup(MethodDefinition method, SetupDeclaration declaration)
    {
        MethodBody body = method.Body;
        body.SimplifyMacros();
        body.InitLocals = true;

        ILProcessor il = body.GetILProcessor();
        VariableDefinition staged = AddVariable(body, runtime.StagingType);
        bool returnsValue = method.ReturnType.MetadataType != MetadataType.Void;
        VariableDefinition? result = returnsValue ? AddVariable(body, method.ReturnType) : null;

        Instruction firstOriginal = body.Instructions[0];
        List<Instruction> prologue = [il.Create(OpCodes.Ldtoken, method)];
        AppendArgumentArray(il, prologue, declaration.Inputs);
        prologue.Add(il.Create(OpCodes.Call, runtime.StageSetup));
        prologue.Add(il.Create(OpCodes.Stloc, staged));
        foreach (Instruction instruction in prologue)
        {
            il.InsertBefore(firstOriginal, instruction);
        }

        List<Instruction> epilogue =
        [
            il.Create(OpCodes.Ldloc, staged),
            il.Create(OpCodes.Call, runtime.CommitSetup),
        ];
        if (result is not null)
        {
            epilogue.Add(il.Create(OpCodes.Ldloc, result));
        }

        epilogue.Add(il.Create(OpCodes.Ret));

        RedirectReturns(il, body, epilogue[0], result);
        foreach (Instruction instruction in epilogue)
        {
            il.Append(instruction);
        }

        body.OptimizeMacros();
    }

    private List<Instruction> BuildPrologue(
        ILProcessor il,
        MethodDefinition method,
        OperationDeclaration declaration,
        VariableDefinition scope)
    {
        // The method's own handle is passed instead of a description of its
        // signature. The runtime reads parameter names, declared types, and
        // nullability from it by reflection, so C# declarations are interpreted
        // in exactly one place.
        List<Instruction> prologue =
        [
            il.Create(OpCodes.Ldtoken, method),
            il.Create(OpCodes.Ldstr, declaration.Operation),
            il.Create(OpCodes.Ldstr, declaration.Component),
            il.Create(OpCodes.Call, runtime.EnterOperation),
            il.Create(OpCodes.Stloc, scope),
        ];

        prologue.Add(il.Create(OpCodes.Ldloc, scope));
        AppendArgumentArray(il, prologue, declaration.Inputs);
        prologue.Add(il.Create(OpCodes.Call, runtime.RecordInputs));

        return prologue;
    }

    /// <summary>Emits <c>new object[] { a, b }</c> from the annotated parameters.</summary>
    private void AppendArgumentArray(
        ILProcessor il, List<Instruction> target, ParameterDefinition[] inputs)
    {
        target.Add(il.Create(OpCodes.Ldc_I4, inputs.Length));
        target.Add(il.Create(OpCodes.Newarr, module.TypeSystem.Object));
        for (int index = 0; index < inputs.Length; index++)
        {
            ParameterDefinition parameter = inputs[index];
            target.Add(il.Create(OpCodes.Dup));
            target.Add(il.Create(OpCodes.Ldc_I4, index));
            target.Add(il.Create(OpCodes.Ldarg, parameter));
            AppendBoxIfNeeded(il, target, parameter.ParameterType);
            target.Add(il.Create(OpCodes.Stelem_Ref));
        }
    }

    private List<Instruction> BuildEpilogue(
        ILProcessor il,
        MethodDefinition method,
        VariableDefinition scope,
        VariableDefinition? result)
    {
        List<Instruction> epilogue = [il.Create(OpCodes.Ldloc, scope)];
        AsyncShape shape = AnnotatedMethod.ShapeOf(method);
        if (shape != AsyncShape.None)
        {
            // The compiler's stub has already started the body, so the operation
            // stays open until the returned awaitable settles. The observer hands
            // back the awaitable the caller must await in its place.
            epilogue.Add(il.Create(OpCodes.Ldloc, result!));
            epilogue.Add(il.Create(OpCodes.Call, runtime.ObserverFor(shape, method.ReturnType)));
        }
        else if (result is null)
        {
            epilogue.Add(il.Create(OpCodes.Call, runtime.ExitVoid));
        }
        else
        {
            epilogue.Add(il.Create(OpCodes.Ldloc, result));
            AppendBoxIfNeeded(il, epilogue, result.VariableType);
            epilogue.Add(il.Create(OpCodes.Call, runtime.ExitOk));
            epilogue.Add(il.Create(OpCodes.Ldloc, result));
        }

        epilogue.Add(il.Create(OpCodes.Ret));
        return epilogue;
    }

    /// <summary>
    /// Turns every <c>ret</c> in the original body into a jump to the epilogue, so
    /// the terminal status is recorded exactly once however the body returns.
    /// </summary>
    /// <remarks>
    /// Each <c>ret</c> is mutated in place rather than replaced. Branch targets and
    /// the boundaries of any exception handler already in the body are instruction
    /// references, so keeping the same instance keeps them valid.
    /// </remarks>
    private static void RedirectReturns(
        ILProcessor il,
        MethodBody body,
        Instruction epilogueStart,
        VariableDefinition? result)
    {
        List<Instruction> returns = [];
        foreach (Instruction instruction in body.Instructions)
        {
            if (instruction.OpCode.Code == Code.Ret)
            {
                returns.Add(instruction);
            }
        }

        foreach (Instruction instruction in returns)
        {
            if (result is null)
            {
                instruction.OpCode = OpCodes.Leave;
                instruction.Operand = epilogueStart;
            }
            else
            {
                instruction.OpCode = OpCodes.Stloc;
                instruction.Operand = result;
                il.InsertAfter(instruction, il.Create(OpCodes.Leave, epilogueStart));
            }
        }
    }

    private void AppendBoxIfNeeded(ILProcessor il, List<Instruction> target, TypeReference type)
    {
        if (RequiresBoxing(type))
        {
            target.Add(il.Create(OpCodes.Box, module.ImportReference(type)));
        }
    }

    private static bool RequiresBoxing(TypeReference type)
    {
        if (type.IsGenericParameter)
        {
            return true;
        }

        if (type.IsValueType)
        {
            return true;
        }

        TypeDefinition? definition = type.Resolve();
        return definition is not null && definition.IsValueType;
    }

    private static VariableDefinition AddVariable(MethodBody body, TypeReference type)
    {
        VariableDefinition variable = new(type);
        body.Variables.Add(variable);
        return variable;
    }
}
