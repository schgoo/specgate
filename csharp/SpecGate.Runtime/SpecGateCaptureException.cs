using System;

namespace SpecGate.Runtime;

/// <summary>
/// Why SpecGate refused to record an observation.
/// </summary>
/// <remarks>
/// These are stable classifications, not message text. A capture that cannot be
/// represented fails loudly rather than recording an approximation, because a
/// silently degraded reference capture would make every later comparison against
/// it meaningless.
/// </remarks>
public enum CaptureRejection
{
    /// <summary>
    /// A declared type has no CTSC representation, such as an unannotated class
    /// or an unsupported collection.
    /// </summary>
    UnsupportedType,

    /// <summary>
    /// The observed value does not inhabit its declared type, such as an object
    /// that does not derive from the declared annotated base class.
    /// </summary>
    TypeMismatch,

    /// <summary>
    /// A null was observed where the declared type is not optional. Declare the
    /// parameter or member nullable to record absence.
    /// </summary>
    NullValue,

    /// <summary>
    /// A map key is not a string. CTSC maps are string-keyed.
    /// </summary>
    UnsupportedMapKey,

    /// <summary>
    /// A value is representable in the CLR but not in CTSC, such as a string
    /// holding an unpaired surrogate.
    /// </summary>
    UnrepresentableValue,

    /// <summary>
    /// An annotated declaration is ambiguous, such as two members of one type
    /// claiming the same semantic name, or a folded surface that would record one
    /// input name twice.
    /// </summary>
    AmbiguousDeclaration,

    /// <summary>
    /// An operation requires provenance from a <c>[SpecSetup]</c> producer that
    /// has not run successfully in this capture.
    /// </summary>
    SetupMissing,

    /// <summary>
    /// Setup provenance cannot be attributed: the setups of one operation
    /// contend for the same contribution, or one declaration ran twice with
    /// different construction inputs.
    /// </summary>
    SetupAmbiguity,

    /// <summary>
    /// The capture sidecar could not be replaced on disk, so the evidence the
    /// command-line tool reads would be missing or stale.
    /// </summary>
    PersistenceFailed,
}

/// <summary>
/// Thrown at an operation boundary when an observation cannot be recorded.
/// </summary>
/// <remarks>
/// This propagates out of the instrumented call, so the test that drove the
/// operation fails. That is deliberate: the alternative is a reference capture
/// that is quietly missing or wrong at one boundary.
/// </remarks>
public sealed class SpecGateCaptureException : Exception
{
    /// <summary>Creates an exception with a default classification.</summary>
    public SpecGateCaptureException()
        : this(CaptureRejection.UnrepresentableValue, "specgate: a value could not be recorded.")
    {
    }

    /// <summary>Creates an exception with a default classification and a message.</summary>
    /// <param name="message">The failure description.</param>
    public SpecGateCaptureException(string message)
        : this(CaptureRejection.UnrepresentableValue, message)
    {
    }

    /// <summary>Creates an exception with a default classification, a message, and a cause.</summary>
    /// <param name="message">The failure description.</param>
    /// <param name="innerException">The underlying cause.</param>
    public SpecGateCaptureException(string message, Exception innerException)
        : base(message, innerException) => Rejection = CaptureRejection.UnrepresentableValue;

    /// <summary>Creates a classified exception.</summary>
    /// <param name="rejection">Why the observation was refused.</param>
    /// <param name="message">The failure description.</param>
    public SpecGateCaptureException(CaptureRejection rejection, string message)
        : base(message) => Rejection = rejection;

    /// <summary>Creates a classified exception with a cause.</summary>
    /// <param name="rejection">Why the observation was refused.</param>
    /// <param name="message">The failure description.</param>
    /// <param name="innerException">The underlying cause.</param>
    public SpecGateCaptureException(CaptureRejection rejection, string message, Exception innerException)
        : base(message, innerException) => Rejection = rejection;

    /// <summary>Gets the stable classification of this failure.</summary>
    public CaptureRejection Rejection { get; }
}
