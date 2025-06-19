from fastapi import FastAPI, Depends, HTTPException, status
from fastapi.middleware.cors import CORSMiddleware
from sqlalchemy.orm import Session
from typing import List
import models, schemas
from database import engine, SessionLocal
from fastapi import Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from starlette import status
from fastapi.staticfiles import StaticFiles

# Create database tables
models.Base.metadata.create_all(bind=engine)

app = FastAPI(title="TaskMark")

@app.exception_handler(RequestValidationError)
async def validation_exception_handler(request: Request, exc: RequestValidationError):
    return JSONResponse(
        status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
        content={
            "detail": exc.errors(),
            "body": exc.body,
        },
    )

# 🌐 CORS setup
origins = [
    "*",
    # "http://adeesh.in",
    # "http://adeesh.in/tasks",
]
app.add_middleware(
    CORSMiddleware,
    allow_origins=origins,
    allow_credentials=True,
    allow_methods=["DELETE, GET, HEAD, OPTIONS, PATCH, POST, PUT"],
    allow_headers=["*"],
)
# :contentReference[oaicite:1]{index=1}

# DB session dependency
def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()

# Create new note/task
@app.post("/notes/", response_model=schemas.TaskNoteRead)
def create_note(note: schemas.TaskNoteCreate, db: Session = Depends(get_db)):
    db_note = models.TaskNote(
        title=note.title,
        content=note.content,
        tags=",".join(note.tags)
    )
    db.add(db_note)
    db.commit()
    db.refresh(db_note)
    return schemas.TaskNoteRead(
        id=db_note.id,
        title=db_note.title,
        content=db_note.content,
        tags=db_note.tags.split(",") if db_note.tags else [],
        created_at=db_note.created_at,
    )

# Read list of notes/tasks
@app.get("/notes/", response_model=List[schemas.TaskNoteRead])
def read_notes(db: Session = Depends(get_db)):
    notes = db.query(models.TaskNote).all()
    return [
        schemas.TaskNoteRead(
            id=n.id,
            title=n.title,
            content=n.content,
            tags=n.tags.split(",") if n.tags else [],
            created_at=n.created_at,
        )
        for n in notes
    ]

# Read single note/task
@app.get("/notes/{note_id}", response_model=schemas.TaskNoteRead)
def read_note(note_id: int, db: Session = Depends(get_db)):
    n = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if not n:
        raise HTTPException(status_code=404, detail="Note not found")
    return schemas.TaskNoteRead(
        id=n.id,
        title=n.title,
        content=n.content,
        tags=n.tags.split(",") if n.tags else [],
        created_at=n.created_at,
    )

# Update a note/task
@app.put("/notes/{note_id}", response_model=schemas.TaskNoteRead)
def update_note(note_id: int, note: schemas.TaskNoteCreate, db: Session = Depends(get_db)):
    n = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if not n:
        raise HTTPException(status_code=404, detail="Note not found")
    n.title = note.title
    n.content = note.content
    n.tags = ",".join(note.tags)
    db.commit()
    db.refresh(n)
    return schemas.TaskNoteRead(
        id=n.id,
        title=n.title,
        content=n.content,
        tags=n.tags.split(",") if n.tags else [],
        created_at=n.created_at,
    )

# Delete a note/task
@app.delete("/notes/{note_id}", status_code=status.HTTP_204_NO_CONTENT)
def delete_note(note_id: int, db: Session = Depends(get_db)):
    n = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if not n:
        raise HTTPException(status_code=404, detail="Note not found")
    db.delete(n)
    db.commit()
    return

# app.mount(
#     "/",
#     StaticFiles(directory="../frontend/dist", html=True),
#     name="frontend"
# )
