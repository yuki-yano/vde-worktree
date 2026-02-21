import { constants as fsConstants } from "node:fs"
import { access, mkdir, open, readFile, rename, writeFile } from "node:fs/promises"
import { dirname } from "node:path"

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
}): Promise<ParsedJsonRecord<T> & { path: string; exists: boolean }> => {
  try {
    await access(path, fsConstants.F_OK)
  } catch {
    return {
      path,
      exists: false,
      valid: true,
      record: null,
    }
  }

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
  } catch {
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
  const tmpPath = `${filePath}.tmp-${String(process.pid)}-${String(Date.now())}`
  await writeFile(tmpPath, `${JSON.stringify(payload)}\n`, "utf8")
  await rename(tmpPath, filePath)
}

export const writeJsonExclusively = async ({
  path,
  payload,
}: {
  readonly path: string
  readonly payload: Record<string, unknown>
}): Promise<boolean> => {
  try {
    const handle = await open(path, "wx")
    await handle.writeFile(`${JSON.stringify(payload)}\n`, "utf8")
    await handle.close()
    return true
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === "EEXIST") {
      return false
    }
    throw error
  }
}
