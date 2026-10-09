import fs from "node:fs"
import path from "node:path"
import { parse as parseJavaScript, type ParserPlugin } from "@babel/parser"
import traverseModule from "@babel/traverse"
import { normalizePath, type Plugin } from "vite"

// @ts-expect-error - CJS/ESM interop.
const traverse = traverseModule.default ?? traverseModule

const SUPPORTED_LOCALES = ["en-US", "tr-TR", "zh-CN", "pt-BR"]
const KEY_PATTERN = /^[a-z0-9_-]+(\.[a-z0-9_-]+)+$/i
const AUTO_IMPORT_NAME = "__staticI18nT"
const AUTO_IMPORT = `import { t as ${AUTO_IMPORT_NAME} } from "/src/i18n/runtime";\n`

const BABEL_PLUGINS = [
  "jsx",
  "typescript",
  ["classProperties", { decoratorsBeforeExport: false }],
  ["classPrivateProperties", { decoratorsBeforeExport: false }],
  "classPrivateMethods",
  "topLevelAwait",
  "importAttributes"
] as unknown as ParserPlugin[]

interface SourceRef {
  file: string
  line: number
  column: number
}

interface MessageRecord {
  id: string
  ref: SourceRef
}

interface CallReplacement {
  start: number
  end: number
}

enum BabelNodeType {
  Identifier = "Identifier",
  StringLiteral = "StringLiteral",
}

function stripQuery(id: string): string {
  return id.split("?")[0]
}

function isSourceFile(id: string): boolean {
  return /\.(?:js|jsx|ts|tsx)$/.test(stripQuery(id))
}

function isExcludedFile(sourceRoot: string, id: string): boolean {
  const cleanID = normalizePath(stripQuery(id))
  if (cleanID.includes("/node_modules/")) return true
  const relativePath = normalizePath(path.relative(sourceRoot, cleanID))
  if (relativePath.startsWith("..")) return true
  return relativePath.startsWith("i18n/")
}

function readJSONFile(filePath: string): Record<string, string> {
  if (!fs.existsSync(filePath)) return {}
  const raw = fs.readFileSync(filePath, "utf8").trim()
  return raw ? JSON.parse(raw) : {}
}

function writeJSONFile(filePath: string, payload: unknown): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, `${JSON.stringify(payload, null, 2)}\n`)
}

function buildRef(
  filePath: string,
  sourceRoot: string,
  loc: { line: number; column: number } | null
): SourceRef {
  return {
    file: normalizePath(path.relative(sourceRoot, filePath)),
    line: loc?.line ?? 1,
    column: loc?.column ?? 1
  }
}

function sourceLocation(
  filePath: string,
  sourceRoot: string,
  loc: { line: number; column: number } | null
): string {
  const ref = buildRef(filePath, sourceRoot, loc)
  return `${ref.file}:${ref.line}:${ref.column}`
}

function validateSourceCall(
  filePath: string,
  sourceRoot: string,
  key: string,
  loc: { line: number; column: number } | null
): void {
  const location = sourceLocation(filePath, sourceRoot, loc)
  if (!KEY_PATTERN.test(key)) {
    throw new Error(
      `[static-i18n] ${location}: i18n key must follow namespace.key format: ${JSON.stringify(key)}`
    )
  }
}

function walkSourceFiles(dirPath: string, visitor: (filePath: string) => void): void {
  for (const entry of fs.readdirSync(dirPath, { withFileTypes: true })) {
    const nextPath = path.join(dirPath, entry.name)
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules") walkSourceFiles(nextPath, visitor)
    } else {
      visitor(nextPath)
    }
  }
}

function parseProgram(code: string, filename: string) {
  try {
    return parseJavaScript(code, {
      sourceType: "module",
      sourceFilename: filename,
      plugins: [...BABEL_PLUGINS]
    })
  } catch (error) {
    throw new Error(
      `[static-i18n] Failed to parse ${filename}: ${(error as Error).message}`
    )
  }
}

function analyzeTCalls(
  code: string,
  filePath: string,
  sourceRoot: string
): { records: MessageRecord[]; replacements: CallReplacement[] } {
  const records: MessageRecord[] = []
  const replacements: CallReplacement[] = []
  const ast = parseProgram(code, filePath)

  traverse(ast, {
    CallExpression(callPath: { node: any; scope: { getBinding(name: string): unknown } }) {
      const node = callPath.node
      if (node.callee.type !== BabelNodeType.Identifier || node.callee.name !== "t") return

      let loc: { line: number; column: number } | null = null
      if (node.loc?.start) {
        loc = { line: node.loc.start.line, column: node.loc.start.column + 1 }
      }
      const location = sourceLocation(filePath, sourceRoot, loc)
      if (callPath.scope.getBinding("t")) {
        throw new Error(
          `[static-i18n] ${location}: t is a reserved global function; remove local binding or import.`
        )
      }
      if (!node.arguments.length) {
        throw new Error(`[static-i18n] ${location}: t() call missing key argument`)
      }

      const firstArg = node.arguments[0]
      if (firstArg.type !== BabelNodeType.StringLiteral || firstArg.value === undefined) {
        throw new Error(`[static-i18n] ${location}: t() key must be a static string literal`)
      }
      validateSourceCall(filePath, sourceRoot, firstArg.value, loc)
      records.push({
        id: firstArg.value,
        ref: buildRef(filePath, sourceRoot, loc)
      })

      const { start, end } = node.callee
      if (start == null || end == null) {
        throw new Error(`[static-i18n] ${location}: unable to locate callee position`)
      }
      replacements.push({ start, end })
    }
  })
  return { records, replacements }
}

function collectCatalog(sourceRoot: string) {
  const records: MessageRecord[] = []
  walkSourceFiles(sourceRoot, (filePath) => {
    const cleanPath = normalizePath(filePath)
    if (!isSourceFile(cleanPath) || isExcludedFile(sourceRoot, cleanPath)) return
    records.push(
      ...analyzeTCalls(fs.readFileSync(cleanPath, "utf8"), cleanPath, sourceRoot).records
    )
  })
  const map: Record<string, { refs: SourceRef[] }> = {}
  for (const r of records) {
    if (!map[r.id]) map[r.id] = { refs: [] }
    map[r.id].refs.push(r.ref)
  }
  const sorted: Record<string, { refs: SourceRef[] }> = {}
  Object.keys(map).sort().forEach(k => {
    sorted[k] = {
      refs: map[k].refs.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line)
    }
  })
  return sorted
}

function validateTranslations(
  sourceRoot: string,
  catalog: Record<string, { refs: SourceRef[] }>,
  requireComplete: boolean
): void {
  for (const locale of SUPPORTED_LOCALES) {
    const localePath = path.join(sourceRoot, "i18n/locales", `${locale}.json`)
    const messages = readJSONFile(localePath)
    for (const id of Object.keys(catalog)) {
      if (!messages[id] && requireComplete) {
        throw new Error(
          `[static-i18n] Missing translation in ${localePath} for key: ${id}`
        )
      }
    }
  }
}

function injectRuntimeImport(code: string, replacements: CallReplacement[]): string {
  let transformed = code
  for (const replacement of replacements.sort((a, b) => b.start - a.start)) {
    transformed =
      transformed.slice(0, replacement.start) +
      AUTO_IMPORT_NAME +
      transformed.slice(replacement.end)
  }
  return AUTO_IMPORT + transformed
}

export function staticI18nPlugin(): Plugin {
  let sourceRoot = path.join(process.cwd(), "src")
  const shouldScan =
    process.argv.includes("--scan") || process.env.STATIC_I18N_SCAN === "true"

  return {
    name: "nexusor-static-i18n",
    enforce: "pre",
    configResolved(config) {
      sourceRoot = path.join(config.root, "src")
    },
    buildStart() {
      const catalog = collectCatalog(sourceRoot)
      if (shouldScan) {
        writeJSONFile(path.join(sourceRoot, "i18n/generated/catalog.json"), catalog)
      }
      validateTranslations(sourceRoot, catalog, !shouldScan)
    },
    transform(code, id) {
      if (!isSourceFile(id) || isExcludedFile(sourceRoot, id)) return null
      const analysis = analyzeTCalls(code, normalizePath(stripQuery(id)), sourceRoot)
      if (!analysis.replacements.length) return null
      return { code: injectRuntimeImport(code, analysis.replacements), map: null }
    }
  }
}
