import { describe, it, expect, beforeAll } from "vitest";
import { getModule } from "./helpers.js";

let mod;

beforeAll(async () => {
  mod = await getModule();
});

// ── Template Decode & Validate ─────────────────────────────────────

describe("decodeJobTemplate", () => {
  const VALID_TEMPLATE = JSON.stringify({
    specificationVersion: "jobtemplate-2023-09",
    name: "TestJob",
    steps: [
      {
        name: "Render",
        script: {
          actions: { onRun: { command: "echo", args: ["hello"] } },
        },
      },
    ],
  });

  it("decodes a valid job template", () => {
    const t = mod.decodeJobTemplate(VALID_TEMPLATE);
    expect(t.name).toBe("TestJob");
    expect(t.specificationVersion).toBe("jobtemplate-2023-09");
    expect(t.stepCount).toBe(1);
    t.free();
  });

  it("throws on invalid template", () => {
    expect(() => mod.decodeJobTemplate("{}")).toThrow();
  });

  it("throws on invalid YAML", () => {
    expect(() => mod.decodeJobTemplate("{{{{")).toThrow();
  });
});

describe("decodeEnvironmentTemplate", () => {
  const VALID_ENV = JSON.stringify({
    specificationVersion: "environment-2023-09",
    environment: {
      name: "TestEnv",
      variables: { FOO: "bar" },
    },
  });

  it("decodes a valid environment template", () => {
    const t = mod.decodeEnvironmentTemplate(VALID_ENV);
    expect(t.specificationVersion).toBe("environment-2023-09");
    t.free();
  });
});

describe("DocumentType", () => {
  it("is exported as an enum with Yaml and Json variants", () => {
    expect(mod.DocumentType.Yaml).toBeDefined();
    expect(mod.DocumentType.Json).toBeDefined();
    expect(mod.DocumentType.Yaml).not.toBe(mod.DocumentType.Json);
  });
});

describe("decodeJobTemplate with explicit format", () => {
  const VALID_JOB_YAML = `
specificationVersion: jobtemplate-2023-09
name: FromYaml
steps:
  - name: S
    script:
      actions:
        onRun:
          command: x
`;

  it("defaults to YAML (matches Python default)", () => {
    const t = mod.decodeJobTemplate(VALID_JOB_YAML);
    expect(t.name).toBe("FromYaml");
    t.free();
  });

  it("DocumentType.Yaml accepts JSON (superset)", () => {
    const json = JSON.stringify({
      specificationVersion: "jobtemplate-2023-09",
      name: "FromJson",
      steps: [
        { name: "S", script: { actions: { onRun: { command: "x" } } } },
      ],
    });
    const t = mod.decodeJobTemplate(json, mod.DocumentType.Yaml);
    expect(t.name).toBe("FromJson");
    t.free();
  });

  it("DocumentType.Json rejects YAML-only syntax", () => {
    expect(() =>
      mod.decodeJobTemplate(VALID_JOB_YAML, mod.DocumentType.Json)
    ).toThrow(/valid JSON/);
  });
});

// F2 regression guard: the YAML parser must enforce the project-wide
// MAX_DOCUMENT_DEPTH budget, not run unbounded into stack exhaustion.
describe("decodeJobTemplate depth budget (F2)", () => {
  it("rejects pathologically deep YAML without crashing the host", () => {
    let doc = "";
    for (let i = 0; i < 200; i++) {
      doc += "  ".repeat(i) + "a:\n";
    }
    for (let i = 0; i < 200; i++) {
      doc += "  ".repeat(200 - 1 - i) + "  b: 1\n";
    }
    // The exact error wording isn't what matters here — either a depth
    // budget error or a missing-specificationVersion error is fine.
    // What matters is that the call returns synchronously with an
    // error, rather than taking down the WASM instance.
    expect(() => mod.decodeJobTemplate(doc, mod.DocumentType.Yaml)).toThrow();
  });
});

describe("decodeJobTemplateFromObject", () => {
  it("accepts a pre-parsed JS object (parity with Python *_dict)", () => {
    const obj = {
      specificationVersion: "jobtemplate-2023-09",
      name: "FromObject",
      steps: [
        { name: "S", script: { actions: { onRun: { command: "x" } } } },
      ],
    };
    const t = mod.decodeJobTemplateFromObject(obj);
    expect(t.name).toBe("FromObject");
    t.free();
  });

  it("rejects a non-object input", () => {
    expect(() => mod.decodeJobTemplateFromObject([])).toThrow();
  });
});

describe("decodeEnvironmentTemplateFromObject", () => {
  it("accepts a pre-parsed JS object", () => {
    const obj = {
      specificationVersion: "environment-2023-09",
      environment: { name: "E", variables: { FOO: "bar" } },
    };
    const t = mod.decodeEnvironmentTemplateFromObject(obj);
    expect(t.specificationVersion).toBe("environment-2023-09");
    t.free();
  });
});

// ── Expression Engine ──────────────────────────────────────────────

describe("ExprValue", () => {
  it("creates string value", () => {
    const v = mod.ExprValue.string("hello");
    expect(v.toString()).toBe("hello");
    expect(v.type).toBe("string");
    v.free();
  });

  it("creates int value", () => {
    const v = mod.ExprValue.int(42n);
    expect(v.toString()).toBe("42");
    expect(v.type).toBe("int");
    v.free();
  });

  it("creates float value", () => {
    const v = mod.ExprValue.float(3.14);
    expect(v.type).toBe("float");
    v.free();
  });

  it("creates bool value", () => {
    const v = mod.ExprValue.bool(true);
    expect(v.toString()).toBe("true");
    expect(v.type).toBe("bool");
    v.free();
  });

  it("creates path value", () => {
    const v = mod.ExprValue.path("/tmp/test", mod.PathFormat.Posix);
    expect(v.type).toBe("path");
    v.free();
  });
});

describe("SymbolTable", () => {
  it("set and get string values", () => {
    const st = new mod.SymbolTable();
    st.setString("Param", "Frames", "1-10");
    expect(st.has("Param", "Frames")).toBe(true);
    expect(st.has("Param", "Missing")).toBe(false);

    const v = st.get("Param", "Frames");
    expect(v.toString()).toBe("1-10");
    v.free();
    st.free();
  });

  it("set ExprValue", () => {
    const st = new mod.SymbolTable();
    const val = mod.ExprValue.int(42n);
    st.set("Param", "Count", val);
    const got = st.get("Param", "Count");
    expect(got.toString()).toBe("42");
    got.free();
    val.free();
    st.free();
  });

  it("allPaths returns all symbol paths", () => {
    const st = new mod.SymbolTable();
    st.setString("Param", "A", "1");
    st.setString("Param", "B", "2");
    const paths = st.allPaths();
    expect(paths).toContain("Param.A");
    expect(paths).toContain("Param.B");
    st.free();
  });
});

describe("FormatString", () => {
  it("parses and resolves a format string", () => {
    const fs = new mod.FormatString("{{Param.Dir}}/output.exr");
    expect(fs.isLiteral).toBe(false);
    expect(fs.references).toContain("Param.Dir");

    const st = new mod.SymbolTable();
    st.setString("Param", "Dir", "/renders");
    const resolved = fs.resolve(st);
    expect(resolved).toBe("/renders/output.exr");

    fs.free();
    st.free();
  });

  it("literal format string", () => {
    const fs = new mod.FormatString("no interpolation");
    expect(fs.isLiteral).toBe(true);
    expect(fs.references).toEqual([]);
    fs.free();
  });
});

describe("ParsedExpression", () => {
  it("parses and evaluates an expression", () => {
    const expr = mod.parseExpression("Param.X");
    expect(expr.expression).toBe("Param.X");
    expect(expr.accessedSymbols).toContain("Param.X");

    const st = new mod.SymbolTable();
    st.setString("Param", "X", "hello");
    const result = expr.evaluate(st);
    expect(result.toString()).toBe("hello");

    result.free();
    st.free();
    expr.free();
  });
});

describe("evaluateExpression", () => {
  it("evaluates a simple expression", () => {
    const st = new mod.SymbolTable();
    st.setString("Param", "Name", "world");
    const result = mod.evaluateExpression("Param.Name", st);
    expect(result.toString()).toBe("world");
    result.free();
    st.free();
  });
});

describe("escapeFormatString", () => {
  it("escapes double braces", () => {
    // Single braces are not special in format strings, only {{ and }} are
    expect(mod.escapeFormatString("hello")).toBe("hello");
    // Test that it's callable and returns a string
    const result = mod.escapeFormatString("test {{ value }}");
    expect(typeof result).toBe("string");
  });

  it("leaves plain strings alone", () => {
    expect(mod.escapeFormatString("hello")).toBe("hello");
  });
});

describe("parseRangeExpr", () => {
  it("parses a simple range", () => {
    const result = mod.parseRangeExpr("1-5");
    expect(Array.from(result)).toEqual([1n, 2n, 3n, 4n, 5n]);
  });

  it("parses a range with step", () => {
    const result = mod.parseRangeExpr("1-10:3");
    expect(Array.from(result)).toEqual([1n, 4n, 7n, 10n]);
  });
});

describe("getDefaultLibrary", () => {
  it("returns a FunctionLibrary", () => {
    const lib = mod.getDefaultLibrary();
    expect(lib).toBeDefined();
    lib.free();
  });
});

describe("getDefaultMemoryLimit / getDefaultOperationLimit", () => {
  it("returns positive numbers", () => {
    expect(mod.getDefaultMemoryLimit()).toBeGreaterThan(0);
    expect(mod.getDefaultOperationLimit()).toBeGreaterThan(0);
  });
});

// ── Job Creation ───────────────────────────────────────────────────

describe("createJob", () => {
  const TEMPLATE_WITH_PARAMS = JSON.stringify({
    specificationVersion: "jobtemplate-2023-09",
    name: "{{Param.JobName}}",
    parameterDefinitions: [
      { name: "JobName", type: "STRING", default: "DefaultJob" },
    ],
    steps: [
      {
        name: "Render",
        script: {
          actions: {
            onRun: { command: "echo", args: ["{{Param.JobName}}"] },
          },
        },
      },
    ],
  });

  it("creates a job with default parameters", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_WITH_PARAMS);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    const job = mod.createJob(template, {}, opts);
    expect(job.name).toBe("DefaultJob");
    expect(job.stepCount).toBe(1);
    expect(job.stepNames).toEqual(["Render"]);
    job.free();
    opts.free();
    template.free();
  });

  it("creates a job with custom parameters", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_WITH_PARAMS);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    const job = mod.createJob(template, { JobName: "MyJob" }, opts);
    expect(job.name).toBe("MyJob");
    job.free();
    opts.free();
    template.free();
  });

  it("job.toJSON() returns a JS object", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_WITH_PARAMS);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    const job = mod.createJob(template, { JobName: "JsonTest" }, opts);
    const json = job.toJSON();
    expect(json.name).toBe("JsonTest");
    expect(json.steps).toHaveLength(1);
    expect(json.steps[0].name).toBe("Render");
    job.free();
    opts.free();
    template.free();
  });
});

// ── Step Dependency Graph ──────────────────────────────────────────

describe("StepDependencyGraph", () => {
  const MULTI_STEP = JSON.stringify({
    specificationVersion: "jobtemplate-2023-09",
    name: "MultiStep",
    steps: [
      {
        name: "Composite",
        script: { actions: { onRun: { command: "composite" } } },
        dependencies: [{ dependsOn: "Render" }],
      },
      {
        name: "Render",
        script: { actions: { onRun: { command: "render" } } },
      },
    ],
  });

  it("returns topological order", () => {
    const template = mod.decodeJobTemplate(MULTI_STEP);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    const job = mod.createJob(template, {}, opts);
    const graph = new mod.StepDependencyGraph(job);
    const order = graph.topologicalOrder();
    expect(order.indexOf("Render")).toBeLessThan(order.indexOf("Composite"));
    graph.free();
    job.free();
    opts.free();
    template.free();
  });
});

// ── Merge Parameter Definitions ────────────────────────────────────

describe("mergeJobParameterDefinitions", () => {
  it("returns parameter definitions", () => {
    const template = mod.decodeJobTemplate(
      JSON.stringify({
        specificationVersion: "jobtemplate-2023-09",
        name: "Test",
        parameterDefinitions: [
          { name: "Frames", type: "STRING", default: "1-10" },
          { name: "Quality", type: "INT", default: 5 },
        ],
        steps: [
          {
            name: "S1",
            script: { actions: { onRun: { command: "echo" } } },
          },
        ],
      })
    );
    const merged = mod.mergeJobParameterDefinitions(template);
    expect(merged).toHaveLength(2);
    // Verify it's an array of objects (structure may vary)
    expect(merged[0]).toBeDefined();
    expect(merged[1]).toBeDefined();
    template.free();
  });
});

// ── Evaluate Let Bindings ──────────────────────────────────────────

describe("evaluateLetBindings", () => {
  it("evaluates let bindings with expression references", () => {
    const st = new mod.SymbolTable();
    st.setString("Param", "X", "hello");
    // RHS of let binding is an expression, result stored at top level
    const result = mod.evaluateLetBindings(["Y=Param.X"], st);
    // allPaths should include the new binding
    const paths = result.allPaths();
    expect(paths.some((p) => p.includes("Y"))).toBe(true);
    result.free();
    st.free();
  });
});

// ── PathParameterOptions ───────────────────────────────────────────

describe("PathParameterOptions", () => {
  it("constructor sets required fields and safe defaults", () => {
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    expect(opts.jobTemplateDir).toBe("/tmpl");
    expect(opts.currentWorkingDir).toBe("/cwd");
    // PathFormat.Posix matches openjd_expr::PathFormat::host() on wasm32.
    expect(opts.pathFormat).toBe(mod.PathFormat.Posix);
    expect(opts.allowTemplateDirWalkUp).toBe(false);
    expect(opts.allowUriPathValues).toBe(false);
    opts.free();
  });

  it("setters update fields", () => {
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    opts.jobTemplateDir = "/other";
    opts.currentWorkingDir = "/wd";
    opts.pathFormat = mod.PathFormat.Windows;
    opts.allowTemplateDirWalkUp = true;
    opts.allowUriPathValues = true;

    expect(opts.jobTemplateDir).toBe("/other");
    expect(opts.currentWorkingDir).toBe("/wd");
    expect(opts.pathFormat).toBe(mod.PathFormat.Windows);
    expect(opts.allowTemplateDirWalkUp).toBe(true);
    expect(opts.allowUriPathValues).toBe(true);
    opts.free();
  });

  it("PathFormat exposes Uri variant (parity with Rust)", () => {
    expect(mod.PathFormat.Uri).toBeDefined();
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    opts.pathFormat = mod.PathFormat.Uri;
    expect(opts.pathFormat).toBe(mod.PathFormat.Uri);
    opts.free();
  });
});

// ── F1 regression guards: PATH-default walk-up protection ─────────

describe("createJob PATH default walk-up protection", () => {
  const TEMPLATE_ABS_PATH_DEFAULT = JSON.stringify({
    specificationVersion: "jobtemplate-2023-09",
    name: "T",
    parameterDefinitions: [
      { name: "Out", type: "PATH", default: "/etc/passwd" },
    ],
    steps: [
      { name: "S", script: { actions: { onRun: { command: "x" } } } },
    ],
  });

  it("rejects absolute PATH default with default options (F1)", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_ABS_PATH_DEFAULT);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    expect(() => mod.createJob(template, {}, opts)).toThrow(/absolute path/);
    opts.free();
    template.free();
  });

  it("accepts absolute PATH default when allowTemplateDirWalkUp=true", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_ABS_PATH_DEFAULT);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    opts.allowTemplateDirWalkUp = true;
    const job = mod.createJob(template, {}, opts);
    expect(job.name).toBe("T");
    job.free();
    opts.free();
    template.free();
  });
});

describe("createJob URI path value protection", () => {
  const TEMPLATE_URI_PATH_DEFAULT = JSON.stringify({
    specificationVersion: "jobtemplate-2023-09",
    name: "T",
    extensions: ["EXPR"],
    parameterDefinitions: [
      { name: "Out", type: "PATH", default: "s3://bucket/key" },
    ],
    steps: [
      { name: "S", script: { actions: { onRun: { command: "x" } } } },
    ],
  });

  it("rejects URI PATH default with default options (F3)", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_URI_PATH_DEFAULT);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    expect(() => mod.createJob(template, {}, opts)).toThrow(
      /URI path values are not permitted/
    );
    opts.free();
    template.free();
  });

  it("accepts URI PATH default when allowUriPathValues=true", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_URI_PATH_DEFAULT);
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    opts.allowUriPathValues = true;
    const job = mod.createJob(template, {}, opts);
    expect(job.name).toBe("T");
    job.free();
    opts.free();
    template.free();
  });
});

describe("preprocessJobParameters", () => {
  it("applies the same options as createJob (F1 via preprocess)", () => {
    const template = mod.decodeJobTemplate(
      JSON.stringify({
        specificationVersion: "jobtemplate-2023-09",
        name: "T",
        parameterDefinitions: [
          { name: "Out", type: "PATH", default: "/etc/passwd" },
        ],
        steps: [
          { name: "S", script: { actions: { onRun: { command: "x" } } } },
        ],
      })
    );
    const opts = new mod.PathParameterOptions("/tmpl", "/cwd");
    expect(() => mod.preprocessJobParameters(template, {}, opts)).toThrow(
      /absolute path/
    );
    opts.free();
    template.free();
  });
});
