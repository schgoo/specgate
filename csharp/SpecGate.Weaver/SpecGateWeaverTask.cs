using System;
using System.Collections.Generic;
using Microsoft.Build.Framework;
using Microsoft.Build.Utilities;

namespace SpecGate.Weaver;

/// <summary>
/// MSBuild task that instruments a compiled assembly's SpecGate operations.
/// </summary>
public sealed class SpecGateWeaverTask : Task
{
    /// <summary>Gets or sets the assembly to weave in place.</summary>
    [Required]
    public string AssemblyPath { get; set; } = string.Empty;

    /// <inheritdoc />
    public override bool Execute()
    {
        try
        {
            IReadOnlyList<string> woven = AssemblyWeaver.Weave(AssemblyPath);
            if (woven.Count == 0)
            {
                Log.LogMessage(
                    MessageImportance.Low, "SpecGate wove no annotated methods into {0}.", AssemblyPath);
                return true;
            }

            Log.LogMessage(
                MessageImportance.Normal,
                "SpecGate wove {0} annotated method(s) into {1}: {2}",
                woven.Count,
                AssemblyPath,
                string.Join(", ", woven));
            return !Log.HasLoggedErrors;
        }
        catch (WeaveException error)
        {
            Log.LogError("SpecGate weaver: {0}", error.Message);
            return false;
        }
        catch (Exception error)
        {
            Log.LogErrorFromException(error, showStackTrace: true);
            return false;
        }
    }
}
