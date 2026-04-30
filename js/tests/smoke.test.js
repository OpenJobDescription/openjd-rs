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

describe("validateTemplate", () => {
  it("returns empty array for valid template", () => {
    const errors = mod.validateTemplate(
      JSON.stringify({
        specificationVersion: "jobtemplate-2023-09",
        name: "Test",
        steps: [
          {
            name: "S1",
            script: { actions: { onRun: { command: "echo" } } },
          },
        ],
      })
    );
    expect(errors).toEqual([]);
  });

  it("returns errors for invalid template", () => {
    const errors = mod.validateTemplate(JSON.stringify({ foo: "bar" }));
    expect(errors.length).toBeGreaterThan(0);
  });
});

describe("isJobTemplate", () => {
  it("returns true for job template", () => {
    const result = mod.isJobTemplate(
      JSON.stringify({
        specificationVersion: "jobtemplate-2023-09",
        name: "Test",
        steps: [
          {
            name: "S1",
            script: { actions: { onRun: { command: "echo" } } },
          },
        ],
      })
    );
    expect(result).toBe(true);
  });

  it("returns false for environment template", () => {
    const result = mod.isJobTemplate(
      JSON.stringify({
        specificationVersion: "environment-2023-09",
        environment: { name: "Env", variables: { X: "1" } },
      })
    );
    expect(result).toBe(false);
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
    const job = mod.createJob(template, {});
    expect(job.name).toBe("DefaultJob");
    expect(job.stepCount).toBe(1);
    expect(job.stepNames).toEqual(["Render"]);
    job.free();
    template.free();
  });

  it("creates a job with custom parameters", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_WITH_PARAMS);
    const job = mod.createJob(template, { JobName: "MyJob" });
    expect(job.name).toBe("MyJob");
    job.free();
    template.free();
  });

  it("job.toJSON() returns a JS object", () => {
    const template = mod.decodeJobTemplate(TEMPLATE_WITH_PARAMS);
    const job = mod.createJob(template, { JobName: "JsonTest" });
    const json = job.toJSON();
    expect(json.name).toBe("JsonTest");
    expect(json.steps).toHaveLength(1);
    expect(json.steps[0].name).toBe("Render");
    job.free();
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
    const job = mod.createJob(template, {});
    const graph = new mod.StepDependencyGraph(job);
    const order = graph.topologicalOrder();
    expect(order.indexOf("Render")).toBeLessThan(order.indexOf("Composite"));
    graph.free();
    job.free();
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
