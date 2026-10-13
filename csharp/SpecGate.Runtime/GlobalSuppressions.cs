using System.Diagnostics.CodeAnalysis;

[assembly: SuppressMessage(
    "Naming",
    "CA1720:Identifier contains type name",
    Scope = "type",
    Target = "~T:SpecGate.Runtime.ValueKind",
    Justification = "Integer, Unsigned, Float, and String are the CTSC value family names. The "
        + "Rust runtime uses the same names, and renaming them here would make the two "
        + "implementations describe the same captured data differently.")]
[assembly: SuppressMessage(
    "Naming",
    "CA1720:Identifier contains type name",
    Scope = "type",
    Target = "~T:SpecGate.Runtime.Value",
    Justification = "The factory methods are named after the CTSC value families they produce, "
        + "matching the Rust runtime's constructors.")]
