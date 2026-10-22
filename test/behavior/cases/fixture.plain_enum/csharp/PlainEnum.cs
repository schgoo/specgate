using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

[SpecEvent("Level")]
// The discriminants deliberately contradict declaration order. CTSC never
// encodes a discriminant, so the published variants must stay Low, Medium,
// High; a producer that ordered by underlying value would reorder them.
public enum Level
{
    [SpecEvent("Low")]
    Low = 30,

    [SpecEvent("Medium")]
    Medium = 10,

    [SpecEvent("High")]
    High = 20,
}

// The holder is named for its surface rather than its file because CA1711
// refuses a type name ending in 'Enum'.
public static class PlainLevels
{
    [SpecOperation("level_for", Spec = "fixture.plain_enum")]
    public static Level LevelFor([SpecInput("score")] int score) =>
        score < 10 ? Level.Low
        : score < 100 ? Level.Medium
        : Level.High;
}
