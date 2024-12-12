#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};
use ic_cdk::caller;


/// Type alias for virtual memory.
pub type Memory = VirtualMemory<DefaultMemoryImpl>;
/// Type alias for a counter to track unique gig IDs.
pub type IdCell = Cell<u64, Memory>;

/// Structure representing a gig/task.
#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
pub struct Gig {
    pub id: u64,                        
    pub title: String,                  
    pub description: String,            
    pub employer: String,                
    pub deadline: u64,                   
    pub assigned_to: Option<String>,     
    pub status: GigStatus,              
    pub created_at: u64,                 
    pub updated_at: Option<u64>,         
}

/// Enum representing possible statuses of a gig.
#[derive(candid::CandidType, Clone, Serialize, Deserialize, PartialEq)]
pub enum GigStatus {
    Open,       // Gig is open and not yet assigned.
    Assigned,   // Gig has been assigned to a worker.
    Approved,  // Gig has been completed by the worker.
    Disputed,   // There is a dispute over the gig.
}

/// Default implementation for `GigStatus` sets the initial status to `Open`.
impl Default for GigStatus {
    fn default() -> Self {
        GigStatus::Open
    }
}

/// Structure for creating or updating a gig.
#[derive(candid::CandidType, Serialize, Deserialize, Default)]
pub struct GigPayload {
    pub title: String,        // Title of the gig.
    pub description: String,  // Description of the gig.
    pub deadline: u64,        // Deadline for gig completion.
}

/// Implement traits for storing `Gig` in stable memory.
impl Storable for Gig {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Gig {
    const MAX_SIZE: u32 = 2048;       // Maximum size for storing a gig.
    const IS_FIXED_SIZE: bool = false; // Indicates that size is not fixed.
}

// Thread-local storage for state management.
thread_local! {
    /// Memory manager for stable memory operations.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    /// Counter to generate unique IDs for gigs.
    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    /// Storage for all gigs using a stable BTreeMap.
    static GIG_STORAGE: RefCell<StableBTreeMap<u64, Gig, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));
}

/// Post a new gig.
#[ic_cdk::update]
pub fn post_gig(payload: GigPayload) -> Gig {
    // Generate a unique ID for the new gig.
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("Cannot increment ID counter");

    // Create a new gig object.
    let gig = Gig {
        id,
        title: payload.title,
        description: payload.description,
        employer: caller().to_string(),
        deadline: payload.deadline,
        assigned_to: None,
        status: GigStatus::Open,
        created_at: time(),
        updated_at: None,
    };

    // Insert the gig into storage.
    do_insert_gig(&gig);
    gig
}

/// Assign a gig to a worker.
#[ic_cdk::update]
pub fn assign_gig(id: u64, worker: String) -> Result<Gig, String> {
    GIG_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        match storage.get(&id) {
            Some(mut gig) => {
                // Ensure only the employer can assign the gig.
                if gig.employer != caller().to_string() {
                    return Err("Only the employer can assign this gig".to_string());
                }
                // Ensure the gig is open before assignment.
                if gig.status != GigStatus::Open {
                    return Err("Gig is not open for assignment".to_string());
                }
                // Update gig details.
                gig.assigned_to = Some(worker);
                gig.status = GigStatus::Assigned;
                gig.updated_at = Some(time());
                storage.insert(gig.id, gig.clone());
                Ok(gig)
            }
            None => Err("Gig not found".to_string()),
        }
    })
}

/// Approve a gig completion.
#[ic_cdk::update]
pub fn approve_gig(id: u64) -> Result<Gig, String> {
    GIG_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        match storage.get(&id) {
            Some(mut gig) => {
                // Ensure only the employer can approve the gig.
                if gig.employer != caller().to_string() {
                    return Err("Only the employer can approve this gig".to_string());
                }
                // Update gig status to approved.
                gig.status = GigStatus::Approved;
                gig.updated_at = Some(time());
                storage.insert(gig.id, gig.clone());
                Ok(gig)
            }
            None => Err("Gig not found".to_string()),
        }
    })
}

/// Update a gig.
#[ic_cdk::update]
pub fn update_gig(id: u64, payload: GigPayload) -> Result<Gig, String> {
    GIG_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        match storage.get(&id) {
            Some(mut gig) => {
                // Ensure only the employer can update the gig.
                if gig.employer != caller().to_string() {
                    return Err("Only the employer can update this gig".to_string());
                }
                // Prevent updates to approved gigs.
                if gig.status == GigStatus::Approved {
                    return Err("Approved gigs cannot be updated".to_string());
                }
                // Update gig details.
                gig.title = payload.title;
                gig.description = payload.description;
                gig.deadline = payload.deadline;
                gig.updated_at = Some(time());
                storage.insert(gig.id, gig.clone());
                Ok(gig)
            }
            None => Err("Gig not found".to_string()),
        }
    })
}

/// Delete a gig.
#[ic_cdk::update]
pub fn delete_gig(id: u64) -> Result<String, String> {
    GIG_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        match storage.get(&id) {
            Some(gig) => {
                // Ensure only the employer can delete the gig.
                if gig.employer != caller().to_string() {
                    return Err("Only the employer can delete this gig".to_string());
                }
                // Remove gig from storage.
                storage.remove(&id);
                Ok("Gig deleted successfully".to_string())
            }
            None => Err("Gig not found".to_string()),
        }
    })
}

/// Retrieve all gigs.
#[ic_cdk::query]
pub fn get_all_gigs() -> Vec<Gig> {
    GIG_STORAGE.with(|storage| storage.borrow().iter().map(|(_, gig)| gig).collect())
}

/// Retrieve a specific gig by ID.
#[ic_cdk::query]
pub fn get_gig(id: u64) -> Option<Gig> {
    GIG_STORAGE.with(|storage| storage.borrow().get(&id))
}

/// Helper function to insert a gig into storage.
fn do_insert_gig(gig: &Gig) {
    GIG_STORAGE.with(|storage| {
        storage.borrow_mut().insert(gig.id, gig.clone());
    });
}

// Export candid interface.
ic_cdk::export_candid!();
