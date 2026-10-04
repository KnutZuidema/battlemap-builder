# Dungeon Forge

An AI-powered 2D battlemap builder for tabletop RPGs. Generate top-down maps from text prompts, edit details, and expand map boundaries with seamless grid-snapped region forging.

## Features

- **Text-to-Map**: Generate high-res 2D top-down battlemaps using OpenRouter AI image models.
- **Region Forging**: Right-drag on the canvas to select any 64px grid-snapped region to edit details, expand map boundaries, or both.
- **Interactive Canvas**: Pan, zoom, adjust selection boxes with resize handles, and inspect request telemetry and token costs in the debug modal.

## Tech Stack

- **Frontend**: React 19, Vite, Tailwind CSS v4, Konva (`react-konva`)
- **Backend**: Node.js, Express, Sharp, OpenRouter API

## Getting Started

### Prerequisites

- Node.js (v18+)
- [OpenRouter](https://openrouter.ai/) API key

### 1. Install Dependencies

```bash
npm install
cd server && npm install
cd ../client && npm install
cd ..
```

### 2. Configure Environment

Create `server/.env` (or copy `server/.env.example`):

```env
PORT=3001
OPENROUTER_API_KEY=your_openrouter_api_key_here
```

### 3. Run Development Server

```bash
npm run dev
```

Open [http://localhost:3000](http://localhost:3000) in your browser. The backend API runs on port 3001.

## License

MIT
