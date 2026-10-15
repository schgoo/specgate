using System;
using System.IO;
using System.Reflection;
using System.Runtime.Loader;

namespace SpecGate.Discovery;

/// <summary>
/// System-call orchestration for discovery, kept behind one narrow adapter so
/// reflection and payload construction do not directly depend on machine
/// filesystem or assembly-loading policy.
/// </summary>
internal sealed class RunnerSystem
{
    /// <summary>The adapter bound to the real filesystem and assembly loader.</summary>
    internal static RunnerSystem Real { get; } = new(
        new FileSystemServices(File.Exists, path => Directory.CreateDirectory(path), File.WriteAllText),
        new AssemblyLoadingServices(
            path => Assembly.LoadFrom(path),
            (context, path) => context.LoadFromAssemblyPath(path)),
        new DependencyResolutionServices(
            path => new AssemblyDependencyResolver(path),
            (resolver, name) => resolver.ResolveAssemblyToPath(name),
            SubscribeDefaultResolver));

    internal Func<string, bool> FileExists { get; }

    internal Func<string, Assembly> LoadAssembly { get; }

    internal Func<AssemblyLoadContext, string, Assembly> LoadFromPath { get; }

    internal Func<string, AssemblyDependencyResolver> CreateResolver { get; }

    internal Func<AssemblyDependencyResolver, AssemblyName, string?> ResolveAssemblyPath { get; }

    internal Func<Func<AssemblyLoadContext, AssemblyName, Assembly?>, IDisposable> SubscribeResolver { get; }

    internal Action<string> CreateDirectory { get; }

    internal Action<string, string> WriteAllText { get; }

    internal RunnerSystem(
        FileSystemServices files,
        AssemblyLoadingServices loading,
        DependencyResolutionServices resolution)
    {
        FileExists = files.FileExists;
        CreateDirectory = files.CreateDirectory;
        WriteAllText = files.WriteAllText;
        LoadAssembly = loading.LoadAssembly;
        LoadFromPath = loading.LoadFromPath;
        CreateResolver = resolution.CreateResolver;
        ResolveAssemblyPath = resolution.ResolveAssemblyPath;
        SubscribeResolver = resolution.SubscribeResolver;
    }

    private static ResolverSubscription SubscribeDefaultResolver(Func<AssemblyLoadContext, AssemblyName, Assembly?> handler)
    {
        AssemblyLoadContext.Default.Resolving += handler;
        return new ResolverSubscription(handler);
    }

    private sealed class ResolverSubscription : IDisposable
    {
        private readonly Func<AssemblyLoadContext, AssemblyName, Assembly?> handler;

        internal ResolverSubscription(Func<AssemblyLoadContext, AssemblyName, Assembly?> handler) =>
            this.handler = handler;

        public void Dispose() => AssemblyLoadContext.Default.Resolving -= handler;
    }
}

/// <summary>Filesystem operations used by discovery.</summary>
internal sealed record FileSystemServices(
    Func<string, bool> FileExists,
    Action<string> CreateDirectory,
    Action<string, string> WriteAllText);

/// <summary>Assembly-loading operations used by discovery.</summary>
internal sealed record AssemblyLoadingServices(
    Func<string, Assembly> LoadAssembly,
    Func<AssemblyLoadContext, string, Assembly> LoadFromPath);

/// <summary>Dependency-resolution operations used by discovery.</summary>
internal sealed record DependencyResolutionServices(
    Func<string, AssemblyDependencyResolver> CreateResolver,
    Func<AssemblyDependencyResolver, AssemblyName, string?> ResolveAssemblyPath,
    Func<Func<AssemblyLoadContext, AssemblyName, Assembly?>, IDisposable> SubscribeResolver);
