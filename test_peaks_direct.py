#!/usr/bin/env python3
import asyncio
import websockets
import json
import sys

async def test():
    try:
        async with websockets.connect('ws://localhost:3008/ws') as ws:
            print("Connected to /ws endpoint")
            print("Waiting for messages (timeout: 5 seconds)...")
            
            # Warte auf Nachrichten mit Timeout
            try:
                message = await asyncio.wait_for(ws.recv(), timeout=5.0)
                data = json.loads(message)
                print(f"Received: {json.dumps(data, indent=2)}")
                
                # Check structure
                if 'peaks' in data:
                    print(f"✓ Peak data found! Flow: {data.get('flow', 'unknown')}")
                    print(f"  Peaks: {data['peaks']}")
                else:
                    print(f"✗ No 'peaks' in data. Structure: {list(data.keys())}")
                    
            except asyncio.TimeoutError:
                print("✗ No messages received in 5 seconds")
                print("This means either:")
                print("  1. No Peak events are being generated")
                print("  2. Peak WebSocket is not forwarding events")
                print("  3. Events are filtered out")
                
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    asyncio.run(test())
