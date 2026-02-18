import { lstat } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"

const hasGitMarker = async (directory: string): Promise<boolean> => {
  try {
    const stat = await lstat(join(directory, ".git"))
    return stat.isDirectory() || stat.isFile()
  } catch {
    return false
  }
}

export const findGitBoundaryDirectory = async (cwd: string): Promise<string | null> => {
  let current = resolve(cwd)
  while (true) {
    if (await hasGitMarker(current)) {
      return current
    }
    const parent = dirname(current)
    if (parent === current) {
      return null
    }
    current = parent
  }
}

export const collectConfigSearchDirectories = async (cwd: string): Promise<readonly string[]> => {
  const absoluteCwd = resolve(cwd)
  const boundary = await findGitBoundaryDirectory(absoluteCwd)
  if (boundary === null) {
    return [absoluteCwd]
  }

  const directories: string[] = []
  let current = absoluteCwd
  while (true) {
    directories.push(current)
    if (current === boundary) {
      break
    }
    const parent = dirname(current)
    if (parent === current) {
      break
    }
    current = parent
  }

  return directories.reverse()
}
