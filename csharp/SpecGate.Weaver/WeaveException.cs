using System;

namespace SpecGate.Weaver;

/// <summary>
/// A condition that stops weaving. The weaver fails the build rather than
/// producing an assembly whose capture coverage is silently incomplete.
/// </summary>
public sealed class WeaveException : Exception
{
    /// <summary>Initializes the exception with a build-facing message.</summary>
    public WeaveException(string message)
        : base(message)
    {
    }

    /// <summary>Initializes the exception with a message and cause.</summary>
    public WeaveException(string message, Exception innerException)
        : base(message, innerException)
    {
    }

    /// <summary>Initializes the exception with no message.</summary>
    public WeaveException()
    {
    }
}
