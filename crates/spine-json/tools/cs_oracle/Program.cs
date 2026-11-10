// C# 오라클: 패키지의 Editor/Core·SpineVersionConverter 소스를 그대로 컴파일한다(.NET 10).
//   dotnet build -c Release -o out
//   dotnet out/oracle.dll dump <in.spine> <outdir> <패키지>/Editor/Schemas 4.0.64 4.1.24 4.2.43 4.3.26
//   dotnet out/oracle.dll num < 입력   (줄마다 "<jd|r2..r5|f|fn> <double 비트>")
// 그 뒤 cargo run -p spine-json --example cs_parity -- <outdir 들의 상위 폴더>

using System;
using System.IO;
using System.Globalization;
using NKStudio.SpineEventInjector.Export;
using NKStudio.SpineEventInjector.Project;

static class Program
{
    static int Main(string[] args)
    {
        CultureInfo.DefaultThreadCurrentCulture = CultureInfo.InvariantCulture;
        if (args[0] == "num")
        {
            // stdin: "kind value" per line, kind: jd (JavaDouble), r<digits>, f (FmtF32 intOk), fn (FmtF32 noint)
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                var p = line.Split(' ');
                double v = BitConverter.Int64BitsToDouble(long.Parse(p[1]));
                string o = p[0] == "jd" ? SpineNumbers.JavaDouble(v)
                    : p[0] == "f" ? SpineNumbers.FmtF32(v)
                    : p[0] == "fn" ? SpineNumbers.FmtF32(v, false)
                    : SpineNumbers.FmtRound(v, int.Parse(p[0].Substring(1)));
                Console.Out.WriteLine(o);
            }
            return 0;
        }
        // dump <in.spine> <outdir> <schemasDir> <versions...>
        var schemas = new System.Collections.Generic.List<SpineProjectSchema>();
        foreach (var f in Directory.GetFiles(args[3], "*.json"))
            schemas.Add(SpineProjectSchema.Parse(Path.GetFileNameWithoutExtension(f), File.ReadAllText(f)));
        var file = SpineProjectFile.LoadForExport(File.ReadAllBytes(args[1]), schemas);
        Directory.CreateDirectory(args[2]);
        int idx = 0;
        foreach (var (name, read) in SkeletonModelReader.Readers(new ProjectView(file)))
        {
            var model = read();
            string stem = Path.Combine(args[2], idx.ToString());
            idx++;
            File.WriteAllText(stem + ".model.json", Newtonsoft.Json.JsonConvert.SerializeObject(model, new BitsConverter()));
            for (int v = 4; v < args.Length; v++)
            {
                foreach (bool ne in new[] { true, false })
                {
                    string outPath = stem + "." + args[v] + "." + (ne ? "ne" : "es");
                    try
                    {
                        File.WriteAllText(outPath + ".json", SkeletonJsonWriter.Write(model, args[v], ne));
                    }
                    catch (Exception e)
                    {
                        File.WriteAllText(outPath + ".err", e.GetType().Name + ": " + e.Message);
                    }
                }
            }
        }
        return 0;
    }
}

sealed class BitsConverter : Newtonsoft.Json.JsonConverter
{
    public override bool CanConvert(Type t) => t == typeof(double);
    public override bool CanRead => false;
    public override object ReadJson(Newtonsoft.Json.JsonReader r, Type t, object e, Newtonsoft.Json.JsonSerializer s) => throw new NotSupportedException();
    public override void WriteJson(Newtonsoft.Json.JsonWriter w, object v, Newtonsoft.Json.JsonSerializer s) =>
        w.WriteValue("d" + BitConverter.DoubleToInt64Bits((double)v).ToString(CultureInfo.InvariantCulture));
}
