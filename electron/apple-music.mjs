const ITUNES_SEARCH_BASE = 'https://itunes.apple.com/search'
const ITUNES_LOOKUP_BASE = 'https://itunes.apple.com/lookup'

function slugify(value) {
  return (value || 'track')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

function buildAppleMusicTrackUrl(track, storefront) {
  const country = (storefront || 'us').toLowerCase()
  const albumSlug = slugify(track.collectionName || track.trackName || 'track')
  return `https://music.apple.com/${country}/album/${albumSlug}/${track.collectionId}?i=${track.trackId}&app=music`
}

function mapSong(song, storefront) {
  return {
    id: String(song.trackId),
    title: song.trackName ?? 'Unknown title',
    artistName: song.artistName ?? 'Unknown artist',
    albumName: song.collectionName ?? '',
    durationMs: song.trackTimeMillis ?? null,
    url: buildAppleMusicTrackUrl(song, storefront),
    artworkUrl: song.artworkUrl100 ?? undefined,
  }
}

function isUrl(value) {
  return /^https?:\/\//i.test(value)
}

function extractAppleMusicTrackId(value) {
  try {
    const url = new URL(value)
    const hostname = url.hostname.toLowerCase()
    if (!hostname.includes('music.apple.com')) {
      return null
    }

    const trackId = url.searchParams.get('i')
    if (!trackId || !/^\d+$/.test(trackId)) {
      return null
    }

    return trackId
  } catch {
    return null
  }
}

function normalized(text) {
  return (text || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim()
}

function tokenSet(text) {
  return new Set(normalized(text).split(/\s+/).filter(Boolean))
}

function overlapRatio(left, right) {
  const leftTokens = [...tokenSet(left)]
  const rightTokenSet = tokenSet(right)
  if (!leftTokens.length) {
    return 0
  }

  const matches = leftTokens.filter((token) => rightTokenSet.has(token)).length
  return matches / leftTokens.length
}

function scoreTrack(query, track) {
  const q = normalized(query)
  const title = normalized(track.trackName)
  const artist = normalized(track.artistName)
  const album = normalized(track.collectionName)
  let score = 0

  if (title === q) score += 300
  if (`${artist} ${title}` === q || `${title} ${artist}` === q) score += 240
  if (artist === q) score += 50
  if (q.includes(title) && title) score += 120
  if (title.includes(q) && q) score += 80
  if (q.includes(artist) && artist) score += 70
  if (artist.includes(q) && q) score += 30
  if (q.includes(album) && album) score += 15

  const terms = q.split(/\s+/).filter(Boolean)
  for (const term of terms) {
    if (title.includes(term)) score += 18
    if (artist.includes(term)) score += 10
    if (album.includes(term)) score += 4
  }

  score += Math.round(overlapRatio(title, q) * 160)
  score += Math.round(overlapRatio(artist, q) * 90)

  if (title && artist && q === `${title} ${artist}`) score += 120
  if (title && artist && q === `${artist} ${title}`) score += 120

  if (!/live|edit|remix|version|karaoke/.test(q) && /live|edit|remix|version|karaoke/.test(title)) {
    score -= 40
  }

  if (!/soundtrack|from /.test(q) && /soundtrack|from /.test(album)) {
    score -= 8
  }

  if (track.primaryGenreName === 'Hip-Hop/Rap') score += 2
  return score
}

export class AppleMusicService {
  async searchTopTrack(query, settings) {
    if (isUrl(query)) {
      const trackId = extractAppleMusicTrackId(query)
      if (!trackId) {
        return null
      }

      return this.lookupTrackById(trackId, settings)
    }

    const results = await this.searchTracks(query, settings)
    return results[0] ?? null
  }

  async lookupTrackById(trackId, settings) {
    const url = new URL(ITUNES_LOOKUP_BASE)
    url.searchParams.set('id', String(trackId))
    url.searchParams.set('country', (settings.storefront || 'us').toUpperCase())
    url.searchParams.set('entity', 'song')

    const response = await fetch(url)

    if (!response.ok) {
      throw new Error(`iTunes Lookup API responded with ${response.status}`)
    }

    const payload = await response.json()
    const song = (payload.results ?? []).find((item) => item.wrapperType === 'track' && item.kind === 'song' && item.trackId)
    return song ? mapSong(song, settings.storefront) : null
  }

  async searchTracks(query, settings) {
    const url = new URL(ITUNES_SEARCH_BASE)
    url.searchParams.set('term', query)
    url.searchParams.set('country', (settings.storefront || 'us').toUpperCase())
    url.searchParams.set('media', 'music')
    url.searchParams.set('entity', 'song')
    url.searchParams.set('limit', '25')
    url.searchParams.set('explicit', 'Yes')

    const response = await fetch(url)

    if (!response.ok) {
      throw new Error(`iTunes Search API responded with ${response.status}`)
    }

    const payload = await response.json()
    const songs = payload.results ?? []

    return songs
      .filter((song) => song.wrapperType === 'track' && song.kind === 'song' && song.trackId && song.collectionId)
      .sort((left, right) => scoreTrack(query, right) - scoreTrack(query, left))
      .map((song) => mapSong(song, settings.storefront))
  }
}
