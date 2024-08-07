export class SingleTypeEventEmitter<T> {
  private listeners: Array<(data: T) => void> = [];

  // Method to add a listener for the event
  on(listener: (data: T) => void) {
    this.listeners.push(listener);
  }

  // Method to remove a listener
  off(listener: (data: T) => void) {
    const index = this.listeners.indexOf(listener);
    if (index > -1) {
      this.listeners.splice(index, 1);
    }
  }

  // Method to emit the event
  emit(data: T) {
    this.listeners.forEach((listener) => listener(data));
  }
}
