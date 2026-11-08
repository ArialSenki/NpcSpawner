using System; using System.IO; using System.Linq; using System.Reflection; using System.Runtime.Loader; using System.Collections.Generic;
// Reads every MSB of the packed game (read only) and writes map, NpcParamId, ThinkParamId, part name, model.
var dir = AppContext.BaseDirectory;
AssemblyLoadContext.Default.Resolving += (c, n) => { var p = Path.Combine(dir, n.Name + ".dll"); return File.Exists(p) ? c.LoadFromAssemblyPath(p) : null; };
Assembly A(string n) => AssemblyLoadContext.Default.LoadFromAssemblyName(new AssemblyName(n));
var io = A("Andre.IO"); var core = A("Andre.Core"); var sf = A("Andre.SoulsFormats");
var vfsT = io.GetType("Andre.IO.VFS.ArchiveBinderVirtualFileSystem");
var gameT = core.GetType("Andre.Core.Game");
var er = Enum.GetNames(gameT).First(x => x == "ER");
dynamic vfs = vfsT.GetMethod("FromGameFolder").Invoke(null, new object[] { Environment.GetEnvironmentVariable("MSB_GAME"), Enum.Parse(gameT, er) });
var names = ((IEnumerable<string>)vfs.GetFileNamesWithExtensions("/map/mapstudio", new[] { ".msb.dcx" })).ToList();
Console.WriteLine($"{names.Count} msb files");
var msbT = sf.GetType("SoulsFormats.MSBE");
var read = msbT.GetMethods(BindingFlags.Public | BindingFlags.Static | BindingFlags.FlattenHierarchy)
  .Where(m => m.Name == "Read" && m.GetParameters().Length == 1).ToList();
foreach (var r in read) Console.WriteLine("Read(" + r.GetParameters()[0].ParameterType + ")");
var readMem = read.FirstOrDefault(m => m.GetParameters()[0].ParameterType == typeof(Memory<byte>));
var readArr = read.FirstOrDefault(m => m.GetParameters()[0].ParameterType == typeof(byte[]));
var rows = new List<string>(); int fail = 0;
foreach (var n in names) {
  try {
    Memory<byte> data = vfs.ReadFileOrThrow(n);
    object msb = readMem != null ? readMem.Invoke(null, new object[] { data }) : readArr.Invoke(null, new object[] { data.ToArray() });
    dynamic m = msb;
    var map = Path.GetFileName(n).Replace(".msb.dcx", "");
    foreach (dynamic e in m.Parts.Enemies) rows.Add($"{map}\t{e.NPCParamID}\t{e.ThinkParamID}\t{e.Name}\t{e.ModelName}");
  } catch (Exception ex) { if (fail++ < 5) Console.WriteLine($"{n}: {(ex.InnerException ?? ex).GetType().Name} {(ex.InnerException ?? ex).Message}"); }
}
File.WriteAllLines(Environment.GetEnvironmentVariable("MSB_OUT"), rows);
Console.WriteLine($"{rows.Count} enemies, {fail} failed");
