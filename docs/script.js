/**
 * iPhone Backup Sync - Landing Page Interactive Script
 * Features:
 * 1. Two-state accessible dark/light theme switch respecting system preference
 * 2. Dynamic GitHub latest release asset fetcher with graceful fallbacks
 * 3. Dynamic GitHub star count fetcher
 * 4. Accessible Copy-to-Clipboard with Invoker Command & fallback support
 * 5. Mobile navigation drawer toggle
 */

;(function () {
  'use strict'

  // ---------------------------------------------------------------------------
  // 1. Two-State Theme Toggle (Modern Web Guidance)
  // ---------------------------------------------------------------------------
  const colorSchemeMeta = document.querySelector('meta[name="color-scheme"]')
  const themeToggleBtn = document.getElementById('theme-toggle')
  const darkMediaQuery = window.matchMedia('(prefers-color-scheme: dark)')

  function getSystemPreference() {
    return darkMediaQuery.matches ? 'dark' : 'light'
  }

  function getCurrentlyRenderedScheme() {
    const stored = getStoredTheme()
    return stored ? stored : getSystemPreference()
  }

  function getStoredTheme() {
    try {
      const value = localStorage.getItem('color-scheme')
      return value === 'dark' || value === 'light' ? value : null
    } catch {
      return null
    }
  }

  function setStoredTheme(theme) {
    try {
      if (theme) {
        localStorage.setItem('color-scheme', theme)
      } else {
        localStorage.removeItem('color-scheme')
      }
    } catch {
      // Storage access may throw in restricted sandboxes
    }
  }

  function applyTheme(storedTheme) {
    if (colorSchemeMeta) {
      colorSchemeMeta.content = storedTheme ? storedTheme : 'light dark'
    }
    if (themeToggleBtn) {
      const current = storedTheme ? storedTheme : getSystemPreference()
      const next = current === 'dark' ? 'light' : 'dark'
      themeToggleBtn.setAttribute('aria-label', `Switch to ${next} theme`)
      themeToggleBtn.setAttribute('title', `Switch to ${next} theme`)
    }
  }

  function toggleTheme() {
    const currentRendered = getCurrentlyRenderedScheme()
    const target = currentRendered === 'dark' ? 'light' : 'dark'
    const system = getSystemPreference()

    // If target equals system preference, user is undoing override: clear storage
    if (target === system) {
      setStoredTheme(null)
      applyTheme(null)
    } else {
      setStoredTheme(target)
      applyTheme(target)
    }
  }

  if (themeToggleBtn) {
    themeToggleBtn.addEventListener('click', toggleTheme)
  }

  // Handle cross-tab sync
  window.addEventListener('storage', (event) => {
    if (event.key === 'color-scheme') {
      applyTheme(getStoredTheme())
    }
  })

  // Handle system preference changes
  darkMediaQuery.addEventListener('change', () => {
    const stored = getStoredTheme()
    applyTheme(stored)
  })

  // Initial setup for button labels
  applyTheme(getStoredTheme())

  // ---------------------------------------------------------------------------
  // 2. Dynamic GitHub Latest Release Asset Fetcher
  // ---------------------------------------------------------------------------
  const REPO_OWNER = 'mahdis24'
  const REPO_NAME = 'iphone-backup-sync'
  const RELEASES_API_URL = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`
  const REPO_API_URL = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}`

  const primaryDownloadBtn = document.getElementById('primary-download-btn')
  const footerDownloadBtn = document.getElementById('footer-download-btn')
  const releaseMetaText = document.getElementById('release-meta-text')
  const footerReleaseMeta = document.getElementById('footer-release-meta')

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return ''
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(1024))
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`
  }

  async function fetchLatestRelease() {
    try {
      const response = await fetch(RELEASES_API_URL, {
        headers: { Accept: 'application/vnd.github.v3+json' }
      })

      if (!response.ok) return

      const data = await response.json()
      const tagName = data.tag_name || 'Latest'
      const assets = data.assets || []

      // Find Windows installer asset (.exe or .msi)
      let windowsAsset = assets.find((asset) => {
        const name = asset.name.toLowerCase()
        return name.endsWith('.exe') || name.endsWith('.msi')
      })

      // Format metadata label
      let metaLabel = tagName
      if (windowsAsset && windowsAsset.size) {
        metaLabel = `${tagName} • ${formatBytes(windowsAsset.size)}`
      }

      if (windowsAsset && windowsAsset.browser_download_url) {
        const downloadUrl = windowsAsset.browser_download_url
        if (primaryDownloadBtn) primaryDownloadBtn.href = downloadUrl
        if (footerDownloadBtn) footerDownloadBtn.href = downloadUrl
      }

      if (releaseMetaText) releaseMetaText.textContent = metaLabel
      if (footerReleaseMeta) footerReleaseMeta.textContent = metaLabel
    } catch {
      // Gracefully silent fallback to default GitHub releases link
    }
  }

  // ---------------------------------------------------------------------------
  // 3. Dynamic GitHub Stars Count Fetcher
  // ---------------------------------------------------------------------------
  const ghStarsBadge = document.getElementById('gh-stars')

  function formatStarCount(stars) {
    if (stars >= 1000) {
      return `★ ${(stars / 1000).toFixed(1)}k`
    }
    return `★ ${stars}`
  }

  async function fetchRepoStars() {
    if (!ghStarsBadge) return
    try {
      const response = await fetch(REPO_API_URL, {
        headers: { Accept: 'application/vnd.github.v3+json' }
      })

      if (!response.ok) return

      const data = await response.json()
      if (typeof data.stargazers_count === 'number') {
        ghStarsBadge.textContent = formatStarCount(data.stargazers_count)
      }
    } catch {
      // Keep static text if request fails or rate-limited
    }
  }

  // ---------------------------------------------------------------------------
  // 4. Accessible Copy-to-Clipboard (Modern Web Guidance)
  // ---------------------------------------------------------------------------
  const timers = new WeakMap()
  let lastMessage = ''

  function report(button, ok) {
    const tip = button.nextElementSibling
    if (!tip) return

    clearTimeout(timers.get(button))
    button.removeAttribute('data-copied')
    button.removeAttribute('data-failed')

    let message = ok ? 'Copied!' : 'Copy failed'
    if (message === lastMessage) message += '\u00A0'
    lastMessage = message

    tip.textContent = ''
    void button.offsetWidth // Restart CSS transition

    button.setAttribute(ok ? 'data-copied' : 'data-failed', '')
    requestAnimationFrame(() => {
      tip.textContent = message
    })

    timers.set(
      button,
      setTimeout(() => {
        button.removeAttribute('data-copied')
        button.removeAttribute('data-failed')
        tip.textContent = ''
      }, 2000)
    )
  }

  function legacyCopy(text) {
    const field = Object.assign(document.createElement('textarea'), {
      value: text
    })
    field.setAttribute('readonly', '')
    field.style.cssText = 'position:fixed;inset-block-start:0;opacity:0'
    document.body.append(field)
    field.select()
    try {
      return document.execCommand('copy')
    } catch {
      return false
    } finally {
      field.remove()
    }
  }

  async function copyTextFromTarget(button, target) {
    if (!target) return
    const text =
      target.matches('input, textarea') ? target.value : target.innerText.trim()

    if (!text) return

    try {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        await navigator.clipboard.writeText(text)
      } else {
        if (!legacyCopy(text)) throw new Error('Legacy copy failed')
      }
      report(button, true)
    } catch {
      report(button, false)
    }
  }

  // Native Invoker Commands detection
  if ('CommandEvent' in window) {
    document.addEventListener(
      'command',
      (event) => {
        if (event.command && event.command.startsWith('--copy')) {
          copyTextFromTarget(event.source, event.target)
        }
      },
      true
    )
  } else {
    // Click fallback for browsers without Invoker Commands support
    document.addEventListener('click', (event) => {
      const button = event.target.closest('button[command^="--copy"]')
      if (!button) return
      const targetId = button.getAttribute('commandfor')
      if (!targetId) return
      const target = document.getElementById(targetId)
      if (target) copyTextFromTarget(button, target)
    })
  }

  // ---------------------------------------------------------------------------
  // 5. Mobile Navigation Menu Toggle
  // ---------------------------------------------------------------------------
  const mobileToggleBtn = document.getElementById('mobile-toggle')
  const siteNav = document.getElementById('site-nav')

  if (mobileToggleBtn && siteNav) {
    mobileToggleBtn.addEventListener('click', () => {
      const isOpen = siteNav.classList.toggle('is-open')
      mobileToggleBtn.setAttribute('aria-expanded', String(isOpen))
    })

    // Close mobile nav when clicking a link
    siteNav.querySelectorAll('.nav-link').forEach((link) => {
      link.addEventListener('click', () => {
        siteNav.classList.remove('is-open')
        mobileToggleBtn.setAttribute('aria-expanded', 'false')
      })
    })

    // Close when clicking outside
    document.addEventListener('click', (event) => {
      if (
        siteNav.classList.contains('is-open') &&
        !siteNav.contains(event.target) &&
        !mobileToggleBtn.contains(event.target)
      ) {
        siteNav.classList.remove('is-open')
        mobileToggleBtn.setAttribute('aria-expanded', 'false')
      }
    })
  }

  // ---------------------------------------------------------------------------
  // Initialize Background Data
  // ---------------------------------------------------------------------------
  fetchLatestRelease()
  fetchRepoStars()
})()
