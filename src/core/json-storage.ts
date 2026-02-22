import { mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname } from "node:path"

let atomicWriteSequence = 0

const nextAtomicWriteSuffix = (): string => {
  atomicWriteSequence += 1
  return `${String(process.pid)}-${process.hrtime.bigint().toString(36)}-${String(atomicWriteSequence)}`
}

export type ParsedJsonRecord<T> = {
  readonly valid: boolean
  readonly record: T | null
}

export const parseJsonRecord = <T>({
  content,
  schemaVersion,
  validate,
}: {
  readonly content: string
  readonly schemaVersion: number
  readonly validate: (candidate: Partial<T>) => candidate is T
}): ParsedJsonRecord<T> => {
  try {
    const parsed = JSON.parse(content) as Partial<T> & { readonly schemaVersion?: number }
    if (parsed.schemaVersion !== schemaVersion || validate(parsed) !== true) {
      return {
        valid: false,
        record: null,
      }
    }
    return {
      valid: true,
      record: parsed,
    }
  } catch {
    return {
      valid: false,
      record: null,
    }
  }
}

export const readJsonRecord = async <T>({
  path,
  schemaVersion,
  validate,
}: {
  readonly path: string
  readonly schemaVersion: number
  readonly validate: (candidate: Partial<T>) => candidate is T
}): Promise<ParsedJsonRecord<T> & { readonly path: string; readonly exists: boolean }> => {
  try {
    const content = await readFile(path, "utf8")
    return {
      path,
      exists: true,
      ...parseJsonRecord({
        content,
        schemaVersion,
        validate,
      }),
    }
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === "ENOENT") {
      return {
        path,
        exists: false,
        valid: true,
        record: null,
      }
    }
    return {
      path,
      exists: true,
      valid: false,
      record: null,
    }
  }
}

export const writeJsonAtomically = async ({
  filePath,
  payload,
  ensureDir = false,
}: {
  readonly filePath: string
  readonly payload: Record<string, unknown>
  readonly ensureDir?: boolean
}): Promise<void> => {
  if (ensureDir) {
    await mkdir(dirname(filePath), { recursive: true })
  }
  const tmpPath = `${filePath}.tmp-${nextAtomicWriteSuffix()}`
  try {
    await writeFile(tmpPath, `${JSON.stringify(payload)}\n`, "utf8")
    await rename(tmpPath, filePath)
  } catch (error) {
    try {
      await rm(tmpPath, { force: true })
    } catch {
      // best-effort cleanup; keep original write error
    }
    throw error
  }
}

export const writeJsonExclusively = async ({
  path,
  payload,
}: {
  readonly path: string
  readonly payload: Record<string, unknown>
}): Promise<boolean> => {
  let handle: Awaited<ReturnType<typeof open>> | undefined
  try {
    handle = await open(path, "wx")
    await handle.writeFile(`${JSON.stringify(payload)}\n`, "utf8")
    return true
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === "EEXIST") {
      return false
    }
    throw error
  } finally {
    if (handle !== undefined) {
      await handle.close()
    }
  }
}
