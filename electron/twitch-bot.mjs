import tmi from 'tmi.js'

export class TwitchBotService {
  constructor(stateManager) {
    this.stateManager = stateManager
    this.client = null
  }

  isConnected() {
    return Boolean(this.client)
  }

  async connect() {
    const { twitch, requestLimits } = this.stateManager.state.settings
    if (!twitch.channel || !twitch.botUsername || !twitch.oauthToken) {
      throw new Error('Twitch channel, bot username, and OAuth token are required.')
    }

    if (this.client) {
      await this.disconnect()
    }

    this.client = new tmi.Client({
      identity: {
        username: twitch.botUsername,
        password: twitch.oauthToken,
      },
      channels: [twitch.channel.replace(/^#/, '')],
    })

    this.client.on('message', async (channel, tags, message, self) => {
      if (self) {
        return
      }

      const [command, ...args] = message.trim().split(/\s+/)
      const normalizedCommand = command?.toLowerCase()
      const removeCommand = '!remove'
      const username = tags['display-name'] || tags.username || 'viewer'

      if (normalizedCommand === removeCommand) {
        const result = this.stateManager.removeLatestRequestByUser(username)
        if (result.message) {
          await this.client.say(channel, `@${tags.username} ${result.message}`)
        }
        return
      }

      if (normalizedCommand !== twitch.requestCommand.toLowerCase()) {
        return
      }

      const isPrivileged = Boolean(tags.mod || tags.badges?.broadcaster) && requestLimits.modsBypassLimits
      const result = await this.stateManager.processRequest({
        requestedBy: username,
        query: args.join(' '),
        isPrivileged,
      })

      if (result.message) {
        await this.client.say(channel, `@${tags.username} ${result.message}`)
      }
    })

    this.client.on('connected', (address, port) => {
      this.stateManager.setBotStatus({
        connected: true,
        status: 'Connected',
        detail: `Listening in #${twitch.channel.replace(/^#/, '')} on ${address}:${port}.`,
      })
      this.stateManager.addLog('info', `Twitch bot connected to #${twitch.channel.replace(/^#/, '')}.`)
    })

    this.client.on('disconnected', (reason) => {
      this.stateManager.setBotStatus({
        connected: false,
        status: 'Disconnected',
        detail: reason || 'Connection closed.',
      })
      this.stateManager.addLog('warn', `Twitch bot disconnected: ${reason || 'unknown reason'}.`)
      this.client = null
    })

    await this.client.connect()
  }

  async disconnect() {
    if (!this.client) {
      return
    }

    const client = this.client
    this.client = null
    await client.disconnect()
    this.stateManager.setBotStatus({
      connected: false,
      status: 'Disconnected',
      detail: 'Bot is offline.',
    })
  }
}
