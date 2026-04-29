var wasm_bindgen = (function(exports) {
    let script_src;
    if (typeof document !== 'undefined' && document.currentScript !== null) {
        script_src = new URL(document.currentScript.src, location.href).toString();
    }

    /**
     * A decoded environment template.
     */
    class EnvironmentTemplate {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(EnvironmentTemplate.prototype);
            obj.__wbg_ptr = ptr;
            EnvironmentTemplateFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            EnvironmentTemplateFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_environmenttemplate_free(ptr, 0);
        }
        /**
         * @returns {string}
         */
        get specificationVersion() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.environmenttemplate_specificationVersion(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
    }
    if (Symbol.dispose) EnvironmentTemplate.prototype[Symbol.dispose] = EnvironmentTemplate.prototype.free;
    exports.EnvironmentTemplate = EnvironmentTemplate;

    /**
     * An expression value (string, int, float, bool, path, list, range).
     */
    class ExprValue {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(ExprValue.prototype);
            obj.__wbg_ptr = ptr;
            ExprValueFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            ExprValueFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_exprvalue_free(ptr, 0);
        }
        /**
         * Create a boolean value.
         * @param {boolean} v
         * @returns {ExprValue}
         */
        static bool(v) {
            const ret = wasm.exprvalue_bool(v);
            return ExprValue.__wrap(ret);
        }
        /**
         * Create a float value.
         * @param {number} v
         * @returns {ExprValue}
         */
        static float(v) {
            const ret = wasm.exprvalue_float(v);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return ExprValue.__wrap(ret[0]);
        }
        /**
         * Create an integer value.
         * @param {bigint} v
         * @returns {ExprValue}
         */
        static int(v) {
            const ret = wasm.exprvalue_int(v);
            return ExprValue.__wrap(ret);
        }
        /**
         * Create a path value.
         * @param {string} v
         * @param {PathFormat} format
         * @returns {ExprValue}
         */
        static path(v, format) {
            const ptr0 = passStringToWasm0(v, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.exprvalue_path(ptr0, len0, format);
            return ExprValue.__wrap(ret);
        }
        /**
         * Create a string value.
         * @param {string} v
         * @returns {ExprValue}
         */
        static string(v) {
            const ptr0 = passStringToWasm0(v, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.exprvalue_string(ptr0, len0);
            return ExprValue.__wrap(ret);
        }
        /**
         * Convert to a native JS value via JSON.
         * @returns {any}
         */
        toJSON() {
            const ret = wasm.exprvalue_toJSON(this.__wbg_ptr);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return takeFromExternrefTable0(ret[0]);
        }
        /**
         * Convert to a display string.
         * @returns {string}
         */
        toString() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.exprvalue_toString(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
        /**
         * Get the type name.
         * @returns {string}
         */
        get type() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.exprvalue_type(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
    }
    if (Symbol.dispose) ExprValue.prototype[Symbol.dispose] = ExprValue.prototype.free;
    exports.ExprValue = ExprValue;

    /**
     * A parsed format string (e.g., `"{{Param.Frames}}/output"`).
     */
    class FormatString {
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            FormatStringFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_formatstring_free(ptr, 0);
        }
        /**
         * Get expression names (the parts inside `{{}}`).
         * @returns {string[]}
         */
        get expressionNames() {
            const ret = wasm.formatstring_expressionNames(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
        /**
         * Whether this is a literal string (no interpolations).
         * @returns {boolean}
         */
        get isLiteral() {
            const ret = wasm.formatstring_isLiteral(this.__wbg_ptr);
            return ret !== 0;
        }
        /**
         * @param {string} input
         */
        constructor(input) {
            const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.formatstring_new(ptr0, len0);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            this.__wbg_ptr = ret[0] >>> 0;
            FormatStringFinalization.register(this, this.__wbg_ptr, this);
            return this;
        }
        /**
         * The raw format string text.
         * @returns {string}
         */
        get raw() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.formatstring_raw(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
        /**
         * Get referenced symbol names (e.g., ["Param.Frames"]).
         * @returns {string[]}
         */
        get references() {
            const ret = wasm.formatstring_references(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
        /**
         * Resolve the format string against a symbol table.
         * @param {SymbolTable} symbols
         * @returns {string}
         */
        resolve(symbols) {
            let deferred2_0;
            let deferred2_1;
            try {
                _assertClass(symbols, SymbolTable);
                const ret = wasm.formatstring_resolve(this.__wbg_ptr, symbols.__wbg_ptr);
                var ptr1 = ret[0];
                var len1 = ret[1];
                if (ret[3]) {
                    ptr1 = 0; len1 = 0;
                    throw takeFromExternrefTable0(ret[2]);
                }
                deferred2_0 = ptr1;
                deferred2_1 = len1;
                return getStringFromWasm0(ptr1, len1);
            } finally {
                wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
            }
        }
    }
    if (Symbol.dispose) FormatString.prototype[Symbol.dispose] = FormatString.prototype.free;
    exports.FormatString = FormatString;

    /**
     * Function library for expression evaluation.
     */
    class FunctionLibrary {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(FunctionLibrary.prototype);
            obj.__wbg_ptr = ptr;
            FunctionLibraryFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            FunctionLibraryFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_functionlibrary_free(ptr, 0);
        }
        /**
         * Get the default function library with all builtins.
         * @returns {FunctionLibrary}
         */
        static default() {
            const ret = wasm.functionlibrary_default();
            return FunctionLibrary.__wrap(ret);
        }
        /**
         * Create a library with path mapping rules.
         * @param {PathMappingRule[]} rules
         * @returns {FunctionLibrary}
         */
        static withPathMappingRules(rules) {
            const ptr0 = passArrayJsValueToWasm0(rules, wasm.__wbindgen_malloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.functionlibrary_withPathMappingRules(ptr0, len0);
            return FunctionLibrary.__wrap(ret);
        }
    }
    if (Symbol.dispose) FunctionLibrary.prototype[Symbol.dispose] = FunctionLibrary.prototype.free;
    exports.FunctionLibrary = FunctionLibrary;

    /**
     * A fully instantiated job with all format strings resolved.
     */
    class Job {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(Job.prototype);
            obj.__wbg_ptr = ptr;
            JobFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            JobFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_job_free(ptr, 0);
        }
        /**
         * @returns {string | undefined}
         */
        get description() {
            const ret = wasm.job_description(this.__wbg_ptr);
            let v1;
            if (ret[0] !== 0) {
                v1 = getStringFromWasm0(ret[0], ret[1]).slice();
                wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
            }
            return v1;
        }
        /**
         * @returns {string}
         */
        get name() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.job_name(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
        /**
         * Number of steps.
         * @returns {number}
         */
        get stepCount() {
            const ret = wasm.job_stepCount(this.__wbg_ptr);
            return ret >>> 0;
        }
        /**
         * Get step names.
         * @returns {string[]}
         */
        get stepNames() {
            const ret = wasm.job_stepNames(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
        /**
         * Get the full job as a JS object via serde.
         * @returns {any}
         */
        toJSON() {
            const ret = wasm.job_toJSON(this.__wbg_ptr);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return takeFromExternrefTable0(ret[0]);
        }
    }
    if (Symbol.dispose) Job.prototype[Symbol.dispose] = Job.prototype.free;
    exports.Job = Job;

    /**
     * A decoded job template.
     */
    class JobTemplate {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(JobTemplate.prototype);
            obj.__wbg_ptr = ptr;
            JobTemplateFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            JobTemplateFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_jobtemplate_free(ptr, 0);
        }
        /**
         * @returns {string}
         */
        get name() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.jobtemplate_name(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
        /**
         * Number of parameter definitions.
         * @returns {number}
         */
        get parameterDefinitionCount() {
            const ret = wasm.jobtemplate_parameterDefinitionCount(this.__wbg_ptr);
            return ret >>> 0;
        }
        /**
         * @returns {string}
         */
        get specificationVersion() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.jobtemplate_specificationVersion(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
        /**
         * Number of steps.
         * @returns {number}
         */
        get stepCount() {
            const ret = wasm.jobtemplate_stepCount(this.__wbg_ptr);
            return ret >>> 0;
        }
        /**
         * Get the full template as a JS object via JSON serialization.
         * @returns {any}
         */
        toJSON() {
            const ret = wasm.jobtemplate_toJSON(this.__wbg_ptr);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return takeFromExternrefTable0(ret[0]);
        }
    }
    if (Symbol.dispose) JobTemplate.prototype[Symbol.dispose] = JobTemplate.prototype.free;
    exports.JobTemplate = JobTemplate;

    /**
     * A parsed expression ready for evaluation.
     */
    class ParsedExpression {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(ParsedExpression.prototype);
            obj.__wbg_ptr = ptr;
            ParsedExpressionFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            ParsedExpressionFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_parsedexpression_free(ptr, 0);
        }
        /**
         * Symbol names accessed by this expression.
         * @returns {string[]}
         */
        get accessedSymbols() {
            const ret = wasm.parsedexpression_accessedSymbols(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
        /**
         * Evaluate the expression against symbol tables.
         * @param {SymbolTable} symbols
         * @param {FunctionLibrary | null} [library]
         * @returns {ExprValue}
         */
        evaluate(symbols, library) {
            _assertClass(symbols, SymbolTable);
            let ptr0 = 0;
            if (!isLikeNone(library)) {
                _assertClass(library, FunctionLibrary);
                ptr0 = library.__destroy_into_raw();
            }
            const ret = wasm.parsedexpression_evaluate(this.__wbg_ptr, symbols.__wbg_ptr, ptr0);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return ExprValue.__wrap(ret[0]);
        }
        /**
         * The expression text.
         * @returns {string}
         */
        get expression() {
            let deferred1_0;
            let deferred1_1;
            try {
                const ret = wasm.parsedexpression_expression(this.__wbg_ptr);
                deferred1_0 = ret[0];
                deferred1_1 = ret[1];
                return getStringFromWasm0(ret[0], ret[1]);
            } finally {
                wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
            }
        }
    }
    if (Symbol.dispose) ParsedExpression.prototype[Symbol.dispose] = ParsedExpression.prototype.free;
    exports.ParsedExpression = ParsedExpression;

    /**
     * Path format: Posix or Windows.
     * @enum {0 | 1}
     */
    const PathFormat = Object.freeze({
        Posix: 0, "0": "Posix",
        Windows: 1, "1": "Windows",
    });
    exports.PathFormat = PathFormat;

    /**
     * A path mapping rule for the function library.
     */
    class PathMappingRule {
        static __unwrap(jsValue) {
            if (!(jsValue instanceof PathMappingRule)) {
                return 0;
            }
            return jsValue.__destroy_into_raw();
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            PathMappingRuleFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_pathmappingrule_free(ptr, 0);
        }
        /**
         * @param {PathFormat} source_format
         * @param {string} source_path
         * @param {string} dest_path
         */
        constructor(source_format, source_path, dest_path) {
            const ptr0 = passStringToWasm0(source_path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(dest_path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ret = wasm.pathmappingrule_new(source_format, ptr0, len0, ptr1, len1);
            this.__wbg_ptr = ret >>> 0;
            PathMappingRuleFinalization.register(this, this.__wbg_ptr, this);
            return this;
        }
    }
    if (Symbol.dispose) PathMappingRule.prototype[Symbol.dispose] = PathMappingRule.prototype.free;
    exports.PathMappingRule = PathMappingRule;

    /**
     * Step dependency graph for analyzing execution order.
     */
    class StepDependencyGraph {
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            StepDependencyGraphFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_stepdependencygraph_free(ptr, 0);
        }
        /**
         * Create a dependency graph from a Job.
         * @param {Job} job
         */
        constructor(job) {
            _assertClass(job, Job);
            const ret = wasm.stepdependencygraph_new(job.__wbg_ptr);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            this.__wbg_ptr = ret[0] >>> 0;
            StepDependencyGraphFinalization.register(this, this.__wbg_ptr, this);
            return this;
        }
        /**
         * Get step names in topological (dependency) order.
         * @returns {string[]}
         */
        topologicalOrder() {
            const ret = wasm.stepdependencygraph_topologicalOrder(this.__wbg_ptr);
            if (ret[3]) {
                throw takeFromExternrefTable0(ret[2]);
            }
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
    }
    if (Symbol.dispose) StepDependencyGraph.prototype[Symbol.dispose] = StepDependencyGraph.prototype.free;
    exports.StepDependencyGraph = StepDependencyGraph;

    /**
     * Iterator over task parameter sets in a step's parameter space.
     */
    class StepParameterSpaceIterator {
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            StepParameterSpaceIteratorFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_stepparameterspaceiterator_free(ptr, 0);
        }
        /**
         * Total number of tasks.
         * @returns {number}
         */
        get count() {
            const ret = wasm.stepparameterspaceiterator_count(this.__wbg_ptr);
            return ret >>> 0;
        }
        /**
         * Get a specific task's parameter set as a JS object.
         * @param {number} index
         * @returns {any}
         */
        get(index) {
            const ret = wasm.stepparameterspaceiterator_get(this.__wbg_ptr, index);
            if (ret[2]) {
                throw takeFromExternrefTable0(ret[1]);
            }
            return takeFromExternrefTable0(ret[0]);
        }
        /**
         * Get parameter names.
         * @returns {string[]}
         */
        get names() {
            const ret = wasm.stepparameterspaceiterator_names(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
    }
    if (Symbol.dispose) StepParameterSpaceIterator.prototype[Symbol.dispose] = StepParameterSpaceIterator.prototype.free;
    exports.StepParameterSpaceIterator = StepParameterSpaceIterator;

    /**
     * A symbol table for format string resolution and expression evaluation.
     */
    class SymbolTable {
        static __wrap(ptr) {
            ptr = ptr >>> 0;
            const obj = Object.create(SymbolTable.prototype);
            obj.__wbg_ptr = ptr;
            SymbolTableFinalization.register(obj, obj.__wbg_ptr, obj);
            return obj;
        }
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            SymbolTableFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_symboltable_free(ptr, 0);
        }
        /**
         * Get all symbol paths (e.g., ["Param.Frames", "Param.OutputDir"]).
         * @returns {string[]}
         */
        allPaths() {
            const ret = wasm.symboltable_allPaths(this.__wbg_ptr);
            var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
            return v1;
        }
        /**
         * Get a value by scope and name.
         * @param {string} scope
         * @param {string} name
         * @returns {ExprValue | undefined}
         */
        get(scope, name) {
            const ptr0 = passStringToWasm0(scope, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ret = wasm.symboltable_get(this.__wbg_ptr, ptr0, len0, ptr1, len1);
            return ret === 0 ? undefined : ExprValue.__wrap(ret);
        }
        /**
         * Check if a scoped key exists.
         * @param {string} scope
         * @param {string} name
         * @returns {boolean}
         */
        has(scope, name) {
            const ptr0 = passStringToWasm0(scope, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ret = wasm.symboltable_has(this.__wbg_ptr, ptr0, len0, ptr1, len1);
            return ret !== 0;
        }
        constructor() {
            const ret = wasm.symboltable_new();
            this.__wbg_ptr = ret >>> 0;
            SymbolTableFinalization.register(this, this.__wbg_ptr, this);
            return this;
        }
        /**
         * Set a scoped value: `set("Param", "Frames", ExprValue.string("1-10"))`.
         * @param {string} scope
         * @param {string} name
         * @param {ExprValue} value
         */
        set(scope, name, value) {
            const ptr0 = passStringToWasm0(scope, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            _assertClass(value, ExprValue);
            const ret = wasm.symboltable_set(this.__wbg_ptr, ptr0, len0, ptr1, len1, value.__wbg_ptr);
            if (ret[1]) {
                throw takeFromExternrefTable0(ret[0]);
            }
        }
        /**
         * Set a string value directly: `setString("Param", "Frames", "1-10")`.
         * @param {string} scope
         * @param {string} name
         * @param {string} value
         */
        setString(scope, name, value) {
            const ptr0 = passStringToWasm0(scope, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len2 = WASM_VECTOR_LEN;
            const ret = wasm.symboltable_setString(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
            if (ret[1]) {
                throw takeFromExternrefTable0(ret[0]);
            }
        }
    }
    if (Symbol.dispose) SymbolTable.prototype[Symbol.dispose] = SymbolTable.prototype.free;
    exports.SymbolTable = SymbolTable;

    /**
     * Create a fully resolved Job from a template and parameter values.
     *
     * `params` is a JS object mapping parameter names to string values.
     * @param {JobTemplate} template
     * @param {any} params
     * @returns {Job}
     */
    function createJob(template, params) {
        _assertClass(template, JobTemplate);
        const ret = wasm.createJob(template.__wbg_ptr, params);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return Job.__wrap(ret[0]);
    }
    exports.createJob = createJob;

    /**
     * Decode and validate an environment template from a JSON/YAML string.
     * @param {string} input
     * @returns {EnvironmentTemplate}
     */
    function decodeEnvironmentTemplate(input) {
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.decodeEnvironmentTemplate(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return EnvironmentTemplate.__wrap(ret[0]);
    }
    exports.decodeEnvironmentTemplate = decodeEnvironmentTemplate;

    /**
     * Decode and validate a job template from a JSON/YAML string.
     * @param {string} input
     * @returns {JobTemplate}
     */
    function decodeJobTemplate(input) {
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.decodeJobTemplate(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return JobTemplate.__wrap(ret[0]);
    }
    exports.decodeJobTemplate = decodeJobTemplate;

    /**
     * Escape `{{` and `}}` in a string for literal use in format strings.
     * @param {string} s
     * @returns {string}
     */
    function escapeFormatString(s) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(s, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.escapeFormatString(ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    exports.escapeFormatString = escapeFormatString;

    /**
     * Evaluate an expression string directly.
     * @param {string} expr
     * @param {SymbolTable} symbols
     * @param {FunctionLibrary | null} [library]
     * @returns {ExprValue}
     */
    function evaluateExpression(expr, symbols, library) {
        const ptr0 = passStringToWasm0(expr, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        _assertClass(symbols, SymbolTable);
        let ptr1 = 0;
        if (!isLikeNone(library)) {
            _assertClass(library, FunctionLibrary);
            ptr1 = library.__destroy_into_raw();
        }
        const ret = wasm.evaluateExpression(ptr0, len0, symbols.__wbg_ptr, ptr1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ExprValue.__wrap(ret[0]);
    }
    exports.evaluateExpression = evaluateExpression;

    /**
     * Evaluate let bindings and return an updated symbol table.
     * @param {string[]} bindings
     * @param {SymbolTable} symbols
     * @returns {SymbolTable}
     */
    function evaluateLetBindings(bindings, symbols) {
        const ptr0 = passArrayJsValueToWasm0(bindings, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        _assertClass(symbols, SymbolTable);
        const ret = wasm.evaluateLetBindings(ptr0, len0, symbols.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return SymbolTable.__wrap(ret[0]);
    }
    exports.evaluateLetBindings = evaluateLetBindings;

    /**
     * Get the default function library.
     * @returns {FunctionLibrary}
     */
    function getDefaultLibrary() {
        const ret = wasm.getDefaultLibrary();
        return FunctionLibrary.__wrap(ret);
    }
    exports.getDefaultLibrary = getDefaultLibrary;

    /**
     * Default memory limit for expression evaluation.
     * @returns {number}
     */
    function getDefaultMemoryLimit() {
        const ret = wasm.getDefaultMemoryLimit();
        return ret >>> 0;
    }
    exports.getDefaultMemoryLimit = getDefaultMemoryLimit;

    /**
     * Default operation limit for expression evaluation.
     * @returns {number}
     */
    function getDefaultOperationLimit() {
        const ret = wasm.getDefaultOperationLimit();
        return ret >>> 0;
    }
    exports.getDefaultOperationLimit = getDefaultOperationLimit;

    /**
     * Get the specification version from a template string.
     * @param {string} input
     * @returns {string}
     */
    function getSpecVersion(input) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.getSpecVersion(ptr0, len0);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    exports.getSpecVersion = getSpecVersion;

    /**
     * Check if a template string is a job template (true), environment template (false), or invalid (throws).
     * @param {string} input
     * @returns {boolean}
     */
    function isJobTemplate(input) {
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.isJobTemplate(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    exports.isJobTemplate = isJobTemplate;

    /**
     * Merge parameter definitions from job and environment templates.
     * @param {JobTemplate} template
     * @returns {any}
     */
    function mergeJobParameterDefinitions(template) {
        _assertClass(template, JobTemplate);
        const ret = wasm.mergeJobParameterDefinitions(template.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    exports.mergeJobParameterDefinitions = mergeJobParameterDefinitions;

    /**
     * Parse an expression string for later evaluation.
     * @param {string} expr
     * @returns {ParsedExpression}
     */
    function parseExpression(expr) {
        const ptr0 = passStringToWasm0(expr, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.parseExpression(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ParsedExpression.__wrap(ret[0]);
    }
    exports.parseExpression = parseExpression;

    /**
     * Parse a range expression (e.g., "1-10:2") into an array of integers.
     * @param {string} expr
     * @returns {BigInt64Array}
     */
    function parseRangeExpr(expr) {
        const ptr0 = passStringToWasm0(expr, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.parseRangeExpr(ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayI64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v2;
    }
    exports.parseRangeExpr = parseRangeExpr;

    /**
     * Parse a YAML or JSON string into a JS object.
     * @param {string} input
     * @returns {any}
     */
    function parseYaml(input) {
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.parseYaml(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    exports.parseYaml = parseYaml;

    /**
     * Preprocess raw parameter values into typed values.
     * @param {JobTemplate} template
     * @param {any} raw_values
     * @returns {any}
     */
    function preprocessJobParameters(template, raw_values) {
        _assertClass(template, JobTemplate);
        const ret = wasm.preprocessJobParameters(template.__wbg_ptr, raw_values);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    exports.preprocessJobParameters = preprocessJobParameters;

    /**
     * Validate a template string. Returns an array of structured error objects (empty = valid).
     * Each error has `path` (array of {type, value} elements), `message`, and `severity` fields.
     * @param {string} input
     * @returns {any}
     */
    function validateTemplate(input) {
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.validateTemplate(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    exports.validateTemplate = validateTemplate;

    function __wbg_get_imports() {
        const import0 = {
            __proto__: null,
            __wbg_Error_2e59b1b37a9a34c3: function(arg0, arg1) {
                const ret = Error(getStringFromWasm0(arg0, arg1));
                return ret;
            },
            __wbg_Number_e6ffdb596c888833: function(arg0) {
                const ret = Number(arg0);
                return ret;
            },
            __wbg_String_8564e559799eccda: function(arg0, arg1) {
                const ret = String(arg1);
                const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                const len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg___wbindgen_boolean_get_a86c216575a75c30: function(arg0) {
                const v = arg0;
                const ret = typeof(v) === 'boolean' ? v : undefined;
                return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
            },
            __wbg___wbindgen_debug_string_dd5d2d07ce9e6c57: function(arg0, arg1) {
                const ret = debugString(arg1);
                const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                const len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg___wbindgen_is_function_49868bde5eb1e745: function(arg0) {
                const ret = typeof(arg0) === 'function';
                return ret;
            },
            __wbg___wbindgen_is_object_40c5a80572e8f9d3: function(arg0) {
                const val = arg0;
                const ret = typeof(val) === 'object' && val !== null;
                return ret;
            },
            __wbg___wbindgen_is_string_b29b5c5a8065ba1a: function(arg0) {
                const ret = typeof(arg0) === 'string';
                return ret;
            },
            __wbg___wbindgen_jsval_loose_eq_3a72ae764d46d944: function(arg0, arg1) {
                const ret = arg0 == arg1;
                return ret;
            },
            __wbg___wbindgen_number_get_7579aab02a8a620c: function(arg0, arg1) {
                const obj = arg1;
                const ret = typeof(obj) === 'number' ? obj : undefined;
                getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
            },
            __wbg___wbindgen_string_get_914df97fcfa788f2: function(arg0, arg1) {
                const obj = arg1;
                const ret = typeof(obj) === 'string' ? obj : undefined;
                var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                var len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg___wbindgen_throw_81fc77679af83bc6: function(arg0, arg1) {
                throw new Error(getStringFromWasm0(arg0, arg1));
            },
            __wbg_call_7f2987183bb62793: function() { return handleError(function (arg0, arg1) {
                const ret = arg0.call(arg1);
                return ret;
            }, arguments); },
            __wbg_done_547d467e97529006: function(arg0) {
                const ret = arg0.done;
                return ret;
            },
            __wbg_entries_616b1a459b85be0b: function(arg0) {
                const ret = Object.entries(arg0);
                return ret;
            },
            __wbg_get_4848e350b40afc16: function(arg0, arg1) {
                const ret = arg0[arg1 >>> 0];
                return ret;
            },
            __wbg_get_ed0642c4b9d31ddf: function() { return handleError(function (arg0, arg1) {
                const ret = Reflect.get(arg0, arg1);
                return ret;
            }, arguments); },
            __wbg_get_unchecked_7d7babe32e9e6a54: function(arg0, arg1) {
                const ret = arg0[arg1 >>> 0];
                return ret;
            },
            __wbg_instanceof_ArrayBuffer_ff7c1337a5e3b33a: function(arg0) {
                let result;
                try {
                    result = arg0 instanceof ArrayBuffer;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_instanceof_Uint8Array_4b8da683deb25d72: function(arg0) {
                let result;
                try {
                    result = arg0 instanceof Uint8Array;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_iterator_de403ef31815a3e6: function() {
                const ret = Symbol.iterator;
                return ret;
            },
            __wbg_length_0c32cb8543c8e4c8: function(arg0) {
                const ret = arg0.length;
                return ret;
            },
            __wbg_length_6e821edde497a532: function(arg0) {
                const ret = arg0.length;
                return ret;
            },
            __wbg_new_4f9fafbb3909af72: function() {
                const ret = new Object();
                return ret;
            },
            __wbg_new_99cabae501c0a8a0: function() {
                const ret = new Map();
                return ret;
            },
            __wbg_new_a560378ea1240b14: function(arg0) {
                const ret = new Uint8Array(arg0);
                return ret;
            },
            __wbg_new_f3c9df4f38f3f798: function() {
                const ret = new Array();
                return ret;
            },
            __wbg_next_01132ed6134b8ef5: function(arg0) {
                const ret = arg0.next;
                return ret;
            },
            __wbg_next_b3713ec761a9dbfd: function() { return handleError(function (arg0) {
                const ret = arg0.next();
                return ret;
            }, arguments); },
            __wbg_pathmappingrule_unwrap: function(arg0) {
                const ret = PathMappingRule.__unwrap(arg0);
                return ret;
            },
            __wbg_prototypesetcall_3e05eb9545565046: function(arg0, arg1, arg2) {
                Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
            },
            __wbg_set_08463b1df38a7e29: function(arg0, arg1, arg2) {
                const ret = arg0.set(arg1, arg2);
                return ret;
            },
            __wbg_set_6be42768c690e380: function(arg0, arg1, arg2) {
                arg0[arg1] = arg2;
            },
            __wbg_set_6c60b2e8ad0e9383: function(arg0, arg1, arg2) {
                arg0[arg1 >>> 0] = arg2;
            },
            __wbg_value_7f6052747ccf940f: function(arg0) {
                const ret = arg0.value;
                return ret;
            },
            __wbindgen_cast_0000000000000001: function(arg0) {
                // Cast intrinsic for `F64 -> Externref`.
                const ret = arg0;
                return ret;
            },
            __wbindgen_cast_0000000000000002: function(arg0) {
                // Cast intrinsic for `I64 -> Externref`.
                const ret = arg0;
                return ret;
            },
            __wbindgen_cast_0000000000000003: function(arg0, arg1) {
                // Cast intrinsic for `Ref(String) -> Externref`.
                const ret = getStringFromWasm0(arg0, arg1);
                return ret;
            },
            __wbindgen_cast_0000000000000004: function(arg0) {
                // Cast intrinsic for `U64 -> Externref`.
                const ret = BigInt.asUintN(64, arg0);
                return ret;
            },
            __wbindgen_init_externref_table: function() {
                const table = wasm.__wbindgen_externrefs;
                const offset = table.grow(4);
                table.set(0, undefined);
                table.set(offset + 0, undefined);
                table.set(offset + 1, null);
                table.set(offset + 2, true);
                table.set(offset + 3, false);
            },
        };
        return {
            __proto__: null,
            "./openjd_for_javascript_bg.js": import0,
        };
    }

    const EnvironmentTemplateFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_environmenttemplate_free(ptr >>> 0, 1));
    const ExprValueFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_exprvalue_free(ptr >>> 0, 1));
    const FormatStringFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_formatstring_free(ptr >>> 0, 1));
    const FunctionLibraryFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_functionlibrary_free(ptr >>> 0, 1));
    const JobFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_job_free(ptr >>> 0, 1));
    const JobTemplateFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_jobtemplate_free(ptr >>> 0, 1));
    const ParsedExpressionFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_parsedexpression_free(ptr >>> 0, 1));
    const PathMappingRuleFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_pathmappingrule_free(ptr >>> 0, 1));
    const StepDependencyGraphFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_stepdependencygraph_free(ptr >>> 0, 1));
    const StepParameterSpaceIteratorFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_stepparameterspaceiterator_free(ptr >>> 0, 1));
    const SymbolTableFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_symboltable_free(ptr >>> 0, 1));

    function addToExternrefTable0(obj) {
        const idx = wasm.__externref_table_alloc();
        wasm.__wbindgen_externrefs.set(idx, obj);
        return idx;
    }

    function _assertClass(instance, klass) {
        if (!(instance instanceof klass)) {
            throw new Error(`expected instance of ${klass.name}`);
        }
    }

    function debugString(val) {
        // primitive types
        const type = typeof val;
        if (type == 'number' || type == 'boolean' || val == null) {
            return  `${val}`;
        }
        if (type == 'string') {
            return `"${val}"`;
        }
        if (type == 'symbol') {
            const description = val.description;
            if (description == null) {
                return 'Symbol';
            } else {
                return `Symbol(${description})`;
            }
        }
        if (type == 'function') {
            const name = val.name;
            if (typeof name == 'string' && name.length > 0) {
                return `Function(${name})`;
            } else {
                return 'Function';
            }
        }
        // objects
        if (Array.isArray(val)) {
            const length = val.length;
            let debug = '[';
            if (length > 0) {
                debug += debugString(val[0]);
            }
            for(let i = 1; i < length; i++) {
                debug += ', ' + debugString(val[i]);
            }
            debug += ']';
            return debug;
        }
        // Test for built-in
        const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
        let className;
        if (builtInMatches && builtInMatches.length > 1) {
            className = builtInMatches[1];
        } else {
            // Failed to match the standard '[object ClassName]'
            return toString.call(val);
        }
        if (className == 'Object') {
            // we're a user defined class or Object
            // JSON.stringify avoids problems with cycles, and is generally much
            // easier than looping through ownProperties of `val`.
            try {
                return 'Object(' + JSON.stringify(val) + ')';
            } catch (_) {
                return 'Object';
            }
        }
        // errors
        if (val instanceof Error) {
            return `${val.name}: ${val.message}\n${val.stack}`;
        }
        // TODO we could test for more things here, like `Set`s and `Map`s.
        return className;
    }

    function getArrayI64FromWasm0(ptr, len) {
        ptr = ptr >>> 0;
        return getBigInt64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
    }

    function getArrayJsValueFromWasm0(ptr, len) {
        ptr = ptr >>> 0;
        const mem = getDataViewMemory0();
        const result = [];
        for (let i = ptr; i < ptr + 4 * len; i += 4) {
            result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
        }
        wasm.__externref_drop_slice(ptr, len);
        return result;
    }

    function getArrayU8FromWasm0(ptr, len) {
        ptr = ptr >>> 0;
        return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
    }

    let cachedBigInt64ArrayMemory0 = null;
    function getBigInt64ArrayMemory0() {
        if (cachedBigInt64ArrayMemory0 === null || cachedBigInt64ArrayMemory0.byteLength === 0) {
            cachedBigInt64ArrayMemory0 = new BigInt64Array(wasm.memory.buffer);
        }
        return cachedBigInt64ArrayMemory0;
    }

    let cachedDataViewMemory0 = null;
    function getDataViewMemory0() {
        if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
            cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
        }
        return cachedDataViewMemory0;
    }

    function getStringFromWasm0(ptr, len) {
        ptr = ptr >>> 0;
        return decodeText(ptr, len);
    }

    let cachedUint8ArrayMemory0 = null;
    function getUint8ArrayMemory0() {
        if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
            cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
        }
        return cachedUint8ArrayMemory0;
    }

    function handleError(f, args) {
        try {
            return f.apply(this, args);
        } catch (e) {
            const idx = addToExternrefTable0(e);
            wasm.__wbindgen_exn_store(idx);
        }
    }

    function isLikeNone(x) {
        return x === undefined || x === null;
    }

    function passArrayJsValueToWasm0(array, malloc) {
        const ptr = malloc(array.length * 4, 4) >>> 0;
        for (let i = 0; i < array.length; i++) {
            const add = addToExternrefTable0(array[i]);
            getDataViewMemory0().setUint32(ptr + 4 * i, add, true);
        }
        WASM_VECTOR_LEN = array.length;
        return ptr;
    }

    function passStringToWasm0(arg, malloc, realloc) {
        if (realloc === undefined) {
            const buf = cachedTextEncoder.encode(arg);
            const ptr = malloc(buf.length, 1) >>> 0;
            getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
            WASM_VECTOR_LEN = buf.length;
            return ptr;
        }

        let len = arg.length;
        let ptr = malloc(len, 1) >>> 0;

        const mem = getUint8ArrayMemory0();

        let offset = 0;

        for (; offset < len; offset++) {
            const code = arg.charCodeAt(offset);
            if (code > 0x7F) break;
            mem[ptr + offset] = code;
        }
        if (offset !== len) {
            if (offset !== 0) {
                arg = arg.slice(offset);
            }
            ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
            const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
            const ret = cachedTextEncoder.encodeInto(arg, view);

            offset += ret.written;
            ptr = realloc(ptr, len, offset, 1) >>> 0;
        }

        WASM_VECTOR_LEN = offset;
        return ptr;
    }

    function takeFromExternrefTable0(idx) {
        const value = wasm.__wbindgen_externrefs.get(idx);
        wasm.__externref_table_dealloc(idx);
        return value;
    }

    let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
    cachedTextDecoder.decode();
    function decodeText(ptr, len) {
        return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
    }

    const cachedTextEncoder = new TextEncoder();

    if (!('encodeInto' in cachedTextEncoder)) {
        cachedTextEncoder.encodeInto = function (arg, view) {
            const buf = cachedTextEncoder.encode(arg);
            view.set(buf);
            return {
                read: arg.length,
                written: buf.length
            };
        };
    }

    let WASM_VECTOR_LEN = 0;

    let wasmModule, wasm;
    function __wbg_finalize_init(instance, module) {
        wasm = instance.exports;
        wasmModule = module;
        cachedBigInt64ArrayMemory0 = null;
        cachedDataViewMemory0 = null;
        cachedUint8ArrayMemory0 = null;
        wasm.__wbindgen_start();
        return wasm;
    }

    async function __wbg_load(module, imports) {
        if (typeof Response === 'function' && module instanceof Response) {
            if (typeof WebAssembly.instantiateStreaming === 'function') {
                try {
                    return await WebAssembly.instantiateStreaming(module, imports);
                } catch (e) {
                    const validResponse = module.ok && expectedResponseType(module.type);

                    if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                        console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                    } else { throw e; }
                }
            }

            const bytes = await module.arrayBuffer();
            return await WebAssembly.instantiate(bytes, imports);
        } else {
            const instance = await WebAssembly.instantiate(module, imports);

            if (instance instanceof WebAssembly.Instance) {
                return { instance, module };
            } else {
                return instance;
            }
        }

        function expectedResponseType(type) {
            switch (type) {
                case 'basic': case 'cors': case 'default': return true;
            }
            return false;
        }
    }

    function initSync(module) {
        if (wasm !== undefined) return wasm;


        if (module !== undefined) {
            if (Object.getPrototypeOf(module) === Object.prototype) {
                ({module} = module)
            } else {
                console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
            }
        }

        const imports = __wbg_get_imports();
        if (!(module instanceof WebAssembly.Module)) {
            module = new WebAssembly.Module(module);
        }
        const instance = new WebAssembly.Instance(module, imports);
        return __wbg_finalize_init(instance, module);
    }

    async function __wbg_init(module_or_path) {
        if (wasm !== undefined) return wasm;


        if (module_or_path !== undefined) {
            if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
                ({module_or_path} = module_or_path)
            } else {
                console.warn('using deprecated parameters for the initialization function; pass a single object instead')
            }
        }

        if (module_or_path === undefined && script_src !== undefined) {
            module_or_path = script_src.replace(/\.js$/, "_bg.wasm");
        }
        const imports = __wbg_get_imports();

        if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
            module_or_path = fetch(module_or_path);
        }

        const { instance, module } = await __wbg_load(await module_or_path, imports);

        return __wbg_finalize_init(instance, module);
    }

    return Object.assign(__wbg_init, { initSync }, exports);
})({ __proto__: null });
