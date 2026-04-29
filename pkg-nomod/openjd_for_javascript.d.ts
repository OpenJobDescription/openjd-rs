declare namespace wasm_bindgen {
    /* tslint:disable */
    /* eslint-disable */

    /**
     * A decoded environment template.
     */
    export class EnvironmentTemplate {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        readonly specificationVersion: string;
    }

    /**
     * An expression value (string, int, float, bool, path, list, range).
     */
    export class ExprValue {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Create a boolean value.
         */
        static bool(v: boolean): ExprValue;
        /**
         * Create a float value.
         */
        static float(v: number): ExprValue;
        /**
         * Create an integer value.
         */
        static int(v: bigint): ExprValue;
        /**
         * Create a path value.
         */
        static path(v: string, format: PathFormat): ExprValue;
        /**
         * Create a string value.
         */
        static string(v: string): ExprValue;
        /**
         * Convert to a native JS value via JSON.
         */
        toJSON(): any;
        /**
         * Convert to a display string.
         */
        toString(): string;
        /**
         * Get the type name.
         */
        readonly type: string;
    }

    /**
     * A parsed format string (e.g., `"{{Param.Frames}}/output"`).
     */
    export class FormatString {
        free(): void;
        [Symbol.dispose](): void;
        constructor(input: string);
        /**
         * Resolve the format string against a symbol table.
         */
        resolve(symbols: SymbolTable): string;
        /**
         * Get expression names (the parts inside `{{}}`).
         */
        readonly expressionNames: string[];
        /**
         * Whether this is a literal string (no interpolations).
         */
        readonly isLiteral: boolean;
        /**
         * The raw format string text.
         */
        readonly raw: string;
        /**
         * Get referenced symbol names (e.g., ["Param.Frames"]).
         */
        readonly references: string[];
    }

    /**
     * Function library for expression evaluation.
     */
    export class FunctionLibrary {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Get the default function library with all builtins.
         */
        static default(): FunctionLibrary;
        /**
         * Create a library with path mapping rules.
         */
        static withPathMappingRules(rules: PathMappingRule[]): FunctionLibrary;
    }

    /**
     * A fully instantiated job with all format strings resolved.
     */
    export class Job {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Get the full job as a JS object via serde.
         */
        toJSON(): any;
        readonly description: string | undefined;
        readonly name: string;
        /**
         * Number of steps.
         */
        readonly stepCount: number;
        /**
         * Get step names.
         */
        readonly stepNames: string[];
    }

    /**
     * A decoded job template.
     */
    export class JobTemplate {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Get the full template as a JS object via JSON serialization.
         */
        toJSON(): any;
        readonly name: string;
        /**
         * Number of parameter definitions.
         */
        readonly parameterDefinitionCount: number;
        readonly specificationVersion: string;
        /**
         * Number of steps.
         */
        readonly stepCount: number;
    }

    /**
     * A parsed expression ready for evaluation.
     */
    export class ParsedExpression {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Evaluate the expression against symbol tables.
         */
        evaluate(symbols: SymbolTable, library?: FunctionLibrary | null): ExprValue;
        /**
         * Symbol names accessed by this expression.
         */
        readonly accessedSymbols: string[];
        /**
         * The expression text.
         */
        readonly expression: string;
    }

    /**
     * Path format: Posix or Windows.
     */
    export enum PathFormat {
        Posix = 0,
        Windows = 1,
    }

    /**
     * A path mapping rule for the function library.
     */
    export class PathMappingRule {
        free(): void;
        [Symbol.dispose](): void;
        constructor(source_format: PathFormat, source_path: string, dest_path: string);
    }

    /**
     * Step dependency graph for analyzing execution order.
     */
    export class StepDependencyGraph {
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Create a dependency graph from a Job.
         */
        constructor(job: Job);
        /**
         * Get step names in topological (dependency) order.
         */
        topologicalOrder(): string[];
    }

    /**
     * Iterator over task parameter sets in a step's parameter space.
     */
    export class StepParameterSpaceIterator {
        private constructor();
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Get a specific task's parameter set as a JS object.
         */
        get(index: number): any;
        /**
         * Total number of tasks.
         */
        readonly count: number;
        /**
         * Get parameter names.
         */
        readonly names: string[];
    }

    /**
     * A symbol table for format string resolution and expression evaluation.
     */
    export class SymbolTable {
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Get all symbol paths (e.g., ["Param.Frames", "Param.OutputDir"]).
         */
        allPaths(): string[];
        /**
         * Get a value by scope and name.
         */
        get(scope: string, name: string): ExprValue | undefined;
        /**
         * Check if a scoped key exists.
         */
        has(scope: string, name: string): boolean;
        constructor();
        /**
         * Set a scoped value: `set("Param", "Frames", ExprValue.string("1-10"))`.
         */
        set(scope: string, name: string, value: ExprValue): void;
        /**
         * Set a string value directly: `setString("Param", "Frames", "1-10")`.
         */
        setString(scope: string, name: string, value: string): void;
    }

    /**
     * Create a fully resolved Job from a template and parameter values.
     *
     * `params` is a JS object mapping parameter names to string values.
     */
    export function createJob(template: JobTemplate, params: any): Job;

    /**
     * Decode and validate an environment template from a JSON/YAML string.
     */
    export function decodeEnvironmentTemplate(input: string): EnvironmentTemplate;

    /**
     * Decode and validate a job template from a JSON/YAML string.
     */
    export function decodeJobTemplate(input: string): JobTemplate;

    /**
     * Escape `{{` and `}}` in a string for literal use in format strings.
     */
    export function escapeFormatString(s: string): string;

    /**
     * Evaluate an expression string directly.
     */
    export function evaluateExpression(expr: string, symbols: SymbolTable, library?: FunctionLibrary | null): ExprValue;

    /**
     * Evaluate let bindings and return an updated symbol table.
     */
    export function evaluateLetBindings(bindings: string[], symbols: SymbolTable): SymbolTable;

    /**
     * Get the default function library.
     */
    export function getDefaultLibrary(): FunctionLibrary;

    /**
     * Default memory limit for expression evaluation.
     */
    export function getDefaultMemoryLimit(): number;

    /**
     * Default operation limit for expression evaluation.
     */
    export function getDefaultOperationLimit(): number;

    /**
     * Get the specification version from a template string.
     */
    export function getSpecVersion(input: string): string;

    /**
     * Check if a template string is a job template (true), environment template (false), or invalid (throws).
     */
    export function isJobTemplate(input: string): boolean;

    /**
     * Merge parameter definitions from job and environment templates.
     */
    export function mergeJobParameterDefinitions(template: JobTemplate): any;

    /**
     * Parse an expression string for later evaluation.
     */
    export function parseExpression(expr: string): ParsedExpression;

    /**
     * Parse a range expression (e.g., "1-10:2") into an array of integers.
     */
    export function parseRangeExpr(expr: string): BigInt64Array;

    /**
     * Parse a YAML or JSON string into a JS object.
     */
    export function parseYaml(input: string): any;

    /**
     * Preprocess raw parameter values into typed values.
     */
    export function preprocessJobParameters(template: JobTemplate, raw_values: any): any;

    /**
     * Validate a template string. Returns an array of structured error objects (empty = valid).
     * Each error has `path` (array of {type, value} elements), `message`, and `severity` fields.
     */
    export function validateTemplate(input: string): any;

}
declare type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

declare interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_exprvalue_free: (a: number, b: number) => void;
    readonly __wbg_formatstring_free: (a: number, b: number) => void;
    readonly __wbg_functionlibrary_free: (a: number, b: number) => void;
    readonly __wbg_parsedexpression_free: (a: number, b: number) => void;
    readonly __wbg_pathmappingrule_free: (a: number, b: number) => void;
    readonly __wbg_symboltable_free: (a: number, b: number) => void;
    readonly escapeFormatString: (a: number, b: number) => [number, number];
    readonly evaluateExpression: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly exprvalue_bool: (a: number) => number;
    readonly exprvalue_float: (a: number) => [number, number, number];
    readonly exprvalue_int: (a: bigint) => number;
    readonly exprvalue_path: (a: number, b: number, c: number) => number;
    readonly exprvalue_string: (a: number, b: number) => number;
    readonly exprvalue_toJSON: (a: number) => [number, number, number];
    readonly exprvalue_toString: (a: number) => [number, number];
    readonly exprvalue_type: (a: number) => [number, number];
    readonly formatstring_expressionNames: (a: number) => [number, number];
    readonly formatstring_isLiteral: (a: number) => number;
    readonly formatstring_new: (a: number, b: number) => [number, number, number];
    readonly formatstring_raw: (a: number) => [number, number];
    readonly formatstring_references: (a: number) => [number, number];
    readonly formatstring_resolve: (a: number, b: number) => [number, number, number, number];
    readonly functionlibrary_default: () => number;
    readonly functionlibrary_withPathMappingRules: (a: number, b: number) => number;
    readonly getDefaultLibrary: () => number;
    readonly getDefaultMemoryLimit: () => number;
    readonly getDefaultOperationLimit: () => number;
    readonly parseExpression: (a: number, b: number) => [number, number, number];
    readonly parseRangeExpr: (a: number, b: number) => [number, number, number, number];
    readonly parsedexpression_accessedSymbols: (a: number) => [number, number];
    readonly parsedexpression_evaluate: (a: number, b: number, c: number) => [number, number, number];
    readonly parsedexpression_expression: (a: number) => [number, number];
    readonly pathmappingrule_new: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly symboltable_allPaths: (a: number) => [number, number];
    readonly symboltable_get: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly symboltable_has: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly symboltable_new: () => number;
    readonly symboltable_set: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly symboltable_setString: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly __wbg_environmenttemplate_free: (a: number, b: number) => void;
    readonly __wbg_job_free: (a: number, b: number) => void;
    readonly __wbg_jobtemplate_free: (a: number, b: number) => void;
    readonly __wbg_stepdependencygraph_free: (a: number, b: number) => void;
    readonly __wbg_stepparameterspaceiterator_free: (a: number, b: number) => void;
    readonly createJob: (a: number, b: any) => [number, number, number];
    readonly decodeEnvironmentTemplate: (a: number, b: number) => [number, number, number];
    readonly decodeJobTemplate: (a: number, b: number) => [number, number, number];
    readonly environmenttemplate_specificationVersion: (a: number) => [number, number];
    readonly evaluateLetBindings: (a: number, b: number, c: number) => [number, number, number];
    readonly getSpecVersion: (a: number, b: number) => [number, number, number, number];
    readonly isJobTemplate: (a: number, b: number) => [number, number, number];
    readonly job_description: (a: number) => [number, number];
    readonly job_name: (a: number) => [number, number];
    readonly job_stepCount: (a: number) => number;
    readonly job_stepNames: (a: number) => [number, number];
    readonly job_toJSON: (a: number) => [number, number, number];
    readonly jobtemplate_name: (a: number) => [number, number];
    readonly jobtemplate_parameterDefinitionCount: (a: number) => number;
    readonly jobtemplate_specificationVersion: (a: number) => [number, number];
    readonly jobtemplate_stepCount: (a: number) => number;
    readonly jobtemplate_toJSON: (a: number) => [number, number, number];
    readonly mergeJobParameterDefinitions: (a: number) => [number, number, number];
    readonly parseYaml: (a: number, b: number) => [number, number, number];
    readonly preprocessJobParameters: (a: number, b: any) => [number, number, number];
    readonly stepdependencygraph_new: (a: number) => [number, number, number];
    readonly stepdependencygraph_topologicalOrder: (a: number) => [number, number, number, number];
    readonly stepparameterspaceiterator_count: (a: number) => number;
    readonly stepparameterspaceiterator_get: (a: number, b: number) => [number, number, number];
    readonly stepparameterspaceiterator_names: (a: number) => [number, number];
    readonly validateTemplate: (a: number, b: number) => [number, number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_drop_slice: (a: number, b: number) => void;
    readonly __wbindgen_start: () => void;
}

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
declare function wasm_bindgen (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
