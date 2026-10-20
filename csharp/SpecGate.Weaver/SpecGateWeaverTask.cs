using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
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

    /// <summary>
    /// Gets or sets where to write the machine-readable weave report, or empty to
    /// write none.
    /// </summary>
    /// <remarks>
    /// The report is one JSON object, <c>{"woven_method_count":N}</c>, in UTF-8
    /// without a byte-order mark. It lets a caller tell an assembly that
    /// legitimately declares nothing from a weave that silently did nothing.
    /// </remarks>
    public string ReportPath { get; set; } = string.Empty;

    /// <inheritdoc />
    public override bool Execute()
    {
        try
        {
            IReadOnlyList<string> woven = AssemblyWeaver.Weave(AssemblyPath);
            WriteReport(woven.Count);
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

    private void WriteReport(int wovenMethodCount)
    {
        if (string.IsNullOrEmpty(ReportPath))
        {
            return;
        }

        string? directory = Path.GetDirectoryName(ReportPath);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        string json = "{\"woven_method_count\":"
            + wovenMethodCount.ToString(CultureInfo.InvariantCulture)
            + "}\n";
        File.WriteAllBytes(ReportPath, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false).GetBytes(json));
    }
}
